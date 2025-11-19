use std::{
    io::{self, LineWriter, Stderr, Write},
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use clap::{Parser, Subcommand};
use clap_verbosity_flag::{InfoLevel, Verbosity};
use miette::IntoDiagnostic;
use opentelemetry::{
    InstrumentationScope, Value as OtelValue,
    trace::{SpanId, SpanKind, Status, TracerProvider},
};
use opentelemetry_sdk::{
    Resource,
    trace::{self as sdktrace, SdkTracerProvider, SpanData, SpanExporter},
};
use pixi_build_types::{
    BackendCapabilities, FrontendCapabilities,
    procedures::negotiate_capabilities::NegotiateCapabilitiesParams,
};
use rattler_build::console_utils::{LoggingOutputHandler, get_default_env_filter};
use serde_json::{Map, Number, Value};
use tracing_core::{Event, Field, Subscriber};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{
    Layer, filter::Directive, layer::Context, layer::SubscriberExt, registry::Registry,
    util::SubscriberInitExt,
};

use crate::{protocol::ProtocolInstantiator, server::Server};

const SERVICE_NAME: &str = env!("CARGO_PKG_NAME");

#[allow(missing_docs)]
#[derive(Parser)]
pub struct App {
    /// The subcommand to run.
    #[clap(subcommand)]
    command: Option<Commands>,

    /// The port to expose the json-rpc server on. If not specified will
    /// communicate with stdin/stdout.
    #[clap(long)]
    http_port: Option<u16>,

    /// Enable verbose logging.
    #[command(flatten)]
    verbose: Verbosity<InfoLevel>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Get the capabilities of the backend.
    Capabilities,
}

/// Run the sever on the specified port or over stdin/stdout.
async fn run_server<T: ProtocolInstantiator>(port: Option<u16>, protocol: T) -> miette::Result<()> {
    let server = Server::new(protocol);
    if let Some(port) = port {
        server.run_over_http(port)
    } else {
        // running over stdin/stdout
        server.run().await
    }
}

/// The actual implementation of the main function that runs the CLI.
pub(crate) async fn main_impl<T: ProtocolInstantiator, F: FnOnce(LoggingOutputHandler) -> T>(
    factory: F,
    args: App,
) -> miette::Result<()> {
    // Setup logging
    let log_handler = LoggingOutputHandler::default();

    let mut env_filter =
        get_default_env_filter(args.verbose.log_level_filter()).into_diagnostic()?;
    env_filter =
        env_filter.add_directive(Directive::from_str("pixi_build=warn").into_diagnostic()?);

    let otel_parts = if args.command.is_none() {
        Some(build_otlp_layer().into_diagnostic()?)
    } else {
        None
    };

    let (otel_layer, event_layer, otel_provider) = match otel_parts {
        Some((layer, events, provider)) => (Some(layer), Some(events), Some(provider)),
        None => (None, None, None),
    };

    match (args.command.is_none(), otel_layer, event_layer) {
        (true, Some(layer), Some(events)) => {
            tracing_subscriber::registry()
                .with(layer)
                .with(events)
                .with(env_filter)
                .init();
        }
        (true, Some(layer), None) => {
            tracing_subscriber::registry()
                .with(layer)
                .with(env_filter)
                .init();
        }
        (true, None, _) => {
            tracing_subscriber::registry().with(env_filter).init();
        }
        (false, Some(layer), Some(events)) => {
            tracing_subscriber::registry()
                .with(layer)
                .with(events)
                .with(env_filter)
                .with(log_handler.clone())
                .init();
        }
        (false, Some(layer), None) => {
            tracing_subscriber::registry()
                .with(layer)
                .with(env_filter)
                .with(log_handler.clone())
                .init();
        }
        (false, None, _) => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(log_handler.clone())
                .init();
        }
    }

    let factory = factory(log_handler);

    let result = match args.command {
        None => run_server(args.http_port, factory).await,
        Some(Commands::Capabilities) => {
            let backend_capabilities = capabilities::<T>().await?;
            eprintln!(
                "Supports {}: {}",
                pixi_build_types::procedures::conda_outputs::METHOD_NAME,
                backend_capabilities.provides_conda_outputs()
            );
            eprintln!(
                "Supports {}: {}",
                pixi_build_types::procedures::conda_build_v1::METHOD_NAME,
                backend_capabilities.provides_conda_build_v1()
            );
            eprintln!(
                "Highest project model: {}",
                backend_capabilities.highest_supported_project_model()
            );
            Ok(())
        }
    };

    if let Some(provider) = otel_provider {
        let _ = provider.shutdown();
    }

    result
}

/// The entry point for the CLI which should be called from the backends implementation.
pub async fn main<T: ProtocolInstantiator, F: FnOnce(LoggingOutputHandler) -> T>(
    factory: F,
) -> miette::Result<()> {
    let args = App::parse();
    main_impl(factory, args).await
}

/// The entry point for the CLI which should be called from the backends implementation.
pub async fn main_ext<T: ProtocolInstantiator, F: FnOnce(LoggingOutputHandler) -> T>(
    factory: F,
    args: Vec<String>,
) -> miette::Result<()> {
    let args = App::parse_from(args);
    main_impl(factory, args).await
}

/// Returns the capabilities of the backend.
async fn capabilities<Factory: ProtocolInstantiator>() -> miette::Result<BackendCapabilities> {
    let result = Factory::negotiate_capabilities(NegotiateCapabilitiesParams {
        capabilities: FrontendCapabilities {},
    })
    .await?;

    Ok(result.capabilities)
}

fn build_otlp_layer() -> io::Result<(
    OpenTelemetryLayer<Registry, sdktrace::Tracer>,
    StandaloneEventLayer,
    SdkTracerProvider,
)> {
    let exporter = OtlpTraceExporter::default();
    let resource = Resource::builder().with_service_name(SERVICE_NAME).build();
    let event_layer = StandaloneEventLayer::new(exporter.writer.clone(), resource.clone());
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter)
        .with_resource(resource)
        .build();
    let tracer = provider.tracer(SERVICE_NAME);
    let layer = tracing_opentelemetry::layer().with_tracer(tracer);
    Ok((layer, event_layer, provider))
}

struct OtlpTraceExporter {
    writer: Arc<Mutex<LineWriter<Stderr>>>,
    resource: Resource,
}

impl Default for OtlpTraceExporter {
    fn default() -> Self {
        Self {
            writer: Arc::new(Mutex::new(LineWriter::new(io::stderr()))),
            resource: Resource::builder_empty().build(),
        }
    }
}

impl std::fmt::Debug for OtlpTraceExporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtlpTraceExporter").finish()
    }
}

struct StandaloneEventLayer {
    writer: Arc<Mutex<LineWriter<Stderr>>>,
    resource: Resource,
}

impl StandaloneEventLayer {
    fn new(writer: Arc<Mutex<LineWriter<Stderr>>>, resource: Resource) -> Self {
        Self { writer, resource }
    }

    fn write_event_line(&self, payload: &Value) {
        if payload.is_null() {
            return;
        }
        if let Ok(encoded) = serde_json::to_vec(payload) {
            if let Ok(mut guard) = self.writer.lock() {
                let _ = guard.write_all(&encoded);
                let _ = guard.write_all(b"\n");
                let _ = guard.flush();
            }
        }
    }
}

impl<S> Layer<S> for StandaloneEventLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if let Some(payload) = orphan_event_to_payload(event, &self.resource) {
            self.write_event_line(&payload);
        }
    }
}

fn orphan_event_to_payload(event: &Event<'_>, resource: &Resource) -> Option<Value> {
    use serde_json::Map;

    let metadata = event.metadata();
    if !metadata.target().starts_with("pixi_build") {
        return None;
    }

    let mut fields = EventFieldVisitor::default();
    event.record(&mut fields);

    let mut extra_fields = fields.fields;

    let message = extra_fields
        .remove("message")
        .and_then(|value| value.as_str().map(|s| s.to_owned()))
        .filter(|msg| !msg.is_empty())
        .or_else(|| {
            (!extra_fields.is_empty()).then(|| Value::Object(extra_fields.clone()).to_string())
        })
        .unwrap_or_else(|| metadata.target().to_string());

    let mut attributes = Vec::new();
    attributes.push(string_attribute("level", metadata.level().as_str()));
    attributes.push(string_attribute("target", metadata.target()));

    for (key, value) in extra_fields {
        let string_value = match value {
            Value::String(s) => s,
            _ => value.to_string(),
        };
        attributes.push(string_attribute(format!("fields.{key}"), &string_value));
    }

    let now = SystemTime::now();
    let timestamp = time_to_nanos(now);

    let mut event_value = Map::new();
    event_value.insert("name".into(), Value::String(message));
    event_value.insert("timeUnixNano".into(), Value::String(timestamp.clone()));
    if !attributes.is_empty() {
        event_value.insert("attributes".into(), Value::Array(attributes));
    }

    let span = standalone_span_with_event(metadata.target(), timestamp.clone(), event_value);

    let mut scope = Map::new();
    let mut scope_descriptor = Map::new();
    scope_descriptor.insert("name".into(), Value::String(metadata.target().to_string()));
    scope.insert("scope".into(), Value::Object(scope_descriptor));
    scope.insert("spans".into(), Value::Array(vec![span]));

    let mut resource_span = Map::new();
    resource_span.insert("resource".into(), resource_to_value(resource));
    resource_span.insert(
        "scopeSpans".into(),
        Value::Array(vec![Value::Object(scope)]),
    );

    let mut root = Map::new();
    root.insert(
        "resourceSpans".into(),
        Value::Array(vec![Value::Object(resource_span)]),
    );
    Some(Value::Object(root))
}

fn standalone_span_with_event(
    target: &str,
    timestamp: String,
    event_value: serde_json::Map<String, Value>,
) -> Value {
    use serde_json::Map;
    let mut span = Map::new();
    span.insert("traceId".into(), Value::String(next_trace_id_hex()));
    span.insert("spanId".into(), Value::String(next_span_id_hex()));
    span.insert("name".into(), Value::String(target.to_string()));
    span.insert("kind".into(), Value::Number(Number::from(1)));
    span.insert("startTimeUnixNano".into(), Value::String(timestamp.clone()));
    span.insert("endTimeUnixNano".into(), Value::String(timestamp));
    span.insert(
        "events".into(),
        Value::Array(vec![Value::Object(event_value)]),
    );
    Value::Object(span)
}

fn string_attribute(key: impl Into<String>, value: impl Into<String>) -> Value {
    use serde_json::Map;
    let mut map = Map::new();
    map.insert("key".into(), Value::String(key.into()));
    let mut value_map = Map::new();
    value_map.insert("stringValue".into(), Value::String(value.into()));
    map.insert("value".into(), Value::Object(value_map));
    Value::Object(map)
}

#[derive(Default)]
struct EventFieldVisitor {
    fields: serde_json::Map<String, Value>,
}

impl tracing::field::Visit for EventFieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_string(), Value::String(value.to_string()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields.insert(
            field.name().to_string(),
            Value::String(format!("{value:?}")),
        );
    }
}

static NEXT_TRACE_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_SPAN_ID: AtomicU64 = AtomicU64::new(1);

fn next_trace_id_hex() -> String {
    let low = NEXT_TRACE_ID.fetch_add(1, Ordering::Relaxed) as u128;
    let high = 0xabcdef01u128;
    format!("{:032x}", (high << 64) | low)
}

fn next_span_id_hex() -> String {
    let id = NEXT_SPAN_ID.fetch_add(1, Ordering::Relaxed);
    format!("{:016x}", id)
}

impl OtlpTraceExporter {
    fn write_batch(&self, batch: Vec<SpanData>) -> opentelemetry_sdk::error::OTelSdkResult {
        if batch.is_empty() {
            return Ok(());
        }

        let payload = traces_to_json(&batch, &self.resource);
        let mut guard = self.writer.lock().map_err(|err| {
            opentelemetry_sdk::error::OTelSdkError::InternalFailure(err.to_string())
        })?;

        let encoded = serde_json::to_vec(&payload).map_err(|err| {
            opentelemetry_sdk::error::OTelSdkError::InternalFailure(err.to_string())
        })?;
        guard
            .write_all(&encoded)
            .and_then(|_| guard.write_all(b"\n"))
            .map_err(|err| {
                opentelemetry_sdk::error::OTelSdkError::InternalFailure(err.to_string())
            })?;
        guard
            .flush()
            .map_err(|err| opentelemetry_sdk::error::OTelSdkError::InternalFailure(err.to_string()))
    }
}

impl SpanExporter for OtlpTraceExporter {
    fn export(
        &self,
        batch: Vec<SpanData>,
    ) -> impl std::future::Future<Output = opentelemetry_sdk::error::OTelSdkResult> + Send {
        std::future::ready(self.write_batch(batch))
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.resource = resource.clone();
    }
}

fn traces_to_json(spans: &[SpanData], resource: &Resource) -> Value {
    let resource_spans = spans
        .iter()
        .map(|span| span_to_resource_span(span, resource))
        .collect::<Vec<_>>();
    let mut root = Map::new();
    root.insert("resourceSpans".into(), Value::Array(resource_spans));
    Value::Object(root)
}

fn span_to_resource_span(span: &SpanData, resource: &Resource) -> Value {
    let mut resource_span = Map::new();
    resource_span.insert("resource".into(), resource_to_value(resource));
    resource_span.insert(
        "scopeSpans".into(),
        Value::Array(vec![scope_span_value(span)]),
    );
    if let Some(schema) = resource.schema_url() {
        resource_span.insert("schemaUrl".into(), Value::String(schema.to_string()));
    }
    Value::Object(resource_span)
}

fn resource_to_value(resource: &Resource) -> Value {
    let mut map = Map::new();
    if !resource.is_empty() {
        let attrs = resource
            .iter()
            .map(|(key, value)| key_value(key.to_string(), value))
            .collect::<Vec<_>>();
        map.insert("attributes".into(), Value::Array(attrs));
    }
    Value::Object(map)
}

fn scope_span_value(span: &SpanData) -> Value {
    let mut scope_span = Map::new();
    scope_span.insert(
        "scope".into(),
        instrumentation_scope_value(&span.instrumentation_scope),
    );
    scope_span.insert("spans".into(), Value::Array(vec![span_to_value(span)]));
    if let Some(schema) = span.instrumentation_scope.schema_url() {
        scope_span.insert("schemaUrl".into(), Value::String(schema.to_string()));
    }
    Value::Object(scope_span)
}

fn instrumentation_scope_value(scope: &InstrumentationScope) -> Value {
    let mut map = Map::new();
    map.insert("name".into(), Value::String(scope.name().to_string()));
    if let Some(version) = scope.version() {
        map.insert("version".into(), Value::String(version.to_string()));
    }
    if let Some(schema) = scope.schema_url() {
        map.insert("schemaUrl".into(), Value::String(schema.to_string()));
    }
    let attributes = scope
        .attributes()
        .map(|kv| key_value(kv.key.to_string(), &kv.value))
        .collect::<Vec<_>>();
    if !attributes.is_empty() {
        map.insert("attributes".into(), Value::Array(attributes));
    }
    Value::Object(map)
}

fn span_to_value(span: &SpanData) -> Value {
    let mut map = Map::new();
    map.insert(
        "traceId".into(),
        Value::String(span.span_context.trace_id().to_string()),
    );
    map.insert(
        "spanId".into(),
        Value::String(span.span_context.span_id().to_string()),
    );

    let trace_state = span.span_context.trace_state().header();
    if !trace_state.is_empty() {
        map.insert("traceState".into(), Value::String(trace_state));
    }

    if span.parent_span_id != SpanId::INVALID {
        map.insert(
            "parentSpanId".into(),
            Value::String(span.parent_span_id.to_string()),
        );
    }

    let mut flags = span.span_context.trace_flags().to_u8() as u32;
    flags |= 1 << 8;
    if span.parent_span_is_remote {
        flags |= 1 << 9;
    }
    map.insert("flags".into(), Value::Number(Number::from(flags)));

    map.insert("name".into(), Value::String(span.name.to_string()));
    map.insert(
        "kind".into(),
        Value::Number(Number::from(span_kind_number(&span.span_kind))),
    );
    map.insert(
        "startTimeUnixNano".into(),
        Value::String(time_to_nanos(span.start_time)),
    );
    map.insert(
        "endTimeUnixNano".into(),
        Value::String(time_to_nanos(span.end_time)),
    );

    if !span.attributes.is_empty() {
        map.insert(
            "attributes".into(),
            Value::Array(
                span.attributes
                    .iter()
                    .map(|kv| key_value(kv.key.to_string(), &kv.value))
                    .collect(),
            ),
        );
    }

    if span.dropped_attributes_count > 0 {
        map.insert(
            "droppedAttributesCount".into(),
            Value::Number(Number::from(span.dropped_attributes_count)),
        );
    }

    if !span.events.events.is_empty() {
        map.insert(
            "events".into(),
            Value::Array(
                span.events
                    .events
                    .iter()
                    .map(event_to_value)
                    .collect::<Vec<_>>(),
            ),
        );
    }
    if span.events.dropped_count > 0 {
        map.insert(
            "droppedEventsCount".into(),
            Value::Number(Number::from(span.events.dropped_count)),
        );
    }

    if !span.links.links.is_empty() {
        map.insert(
            "links".into(),
            Value::Array(
                span.links
                    .links
                    .iter()
                    .map(link_to_value)
                    .collect::<Vec<_>>(),
            ),
        );
    }
    if span.links.dropped_count > 0 {
        map.insert(
            "droppedLinksCount".into(),
            Value::Number(Number::from(span.links.dropped_count)),
        );
    }

    if !matches!(span.status, Status::Unset) {
        let mut status = Map::new();
        let (code, message) = status_code_and_message(&span.status);
        status.insert("code".into(), Value::Number(Number::from(code)));
        if let Some(message) = message {
            status.insert("message".into(), Value::String(message));
        }
        map.insert("status".into(), Value::Object(status));
    }

    Value::Object(map)
}

fn event_to_value(event: &opentelemetry::trace::Event) -> Value {
    let mut map = Map::new();
    map.insert("name".into(), Value::String(event.name.to_string()));
    map.insert(
        "timeUnixNano".into(),
        Value::String(time_to_nanos(event.timestamp)),
    );
    if !event.attributes.is_empty() {
        map.insert(
            "attributes".into(),
            Value::Array(
                event
                    .attributes
                    .iter()
                    .map(|kv| key_value(kv.key.to_string(), &kv.value))
                    .collect(),
            ),
        );
    }
    if event.dropped_attributes_count > 0 {
        map.insert(
            "droppedAttributesCount".into(),
            Value::Number(Number::from(event.dropped_attributes_count)),
        );
    }
    Value::Object(map)
}

fn link_to_value(link: &opentelemetry::trace::Link) -> Value {
    let mut map = Map::new();
    map.insert(
        "traceId".into(),
        Value::String(link.span_context.trace_id().to_string()),
    );
    map.insert(
        "spanId".into(),
        Value::String(link.span_context.span_id().to_string()),
    );
    let trace_state = link.span_context.trace_state().header();
    if !trace_state.is_empty() {
        map.insert("traceState".into(), Value::String(trace_state));
    }
    if !link.attributes.is_empty() {
        map.insert(
            "attributes".into(),
            Value::Array(
                link.attributes
                    .iter()
                    .map(|kv| key_value(kv.key.to_string(), &kv.value))
                    .collect(),
            ),
        );
    }
    if link.dropped_attributes_count > 0 {
        map.insert(
            "droppedAttributesCount".into(),
            Value::Number(Number::from(link.dropped_attributes_count)),
        );
    }
    Value::Object(map)
}

fn span_kind_number(kind: &SpanKind) -> u8 {
    match kind {
        SpanKind::Client => 3,
        SpanKind::Server => 2,
        SpanKind::Producer => 4,
        SpanKind::Consumer => 5,
        SpanKind::Internal => 1,
        #[allow(unreachable_patterns)]
        _ => 0,
    }
}

fn status_code_and_message(status: &Status) -> (u8, Option<String>) {
    match status {
        Status::Unset => (0, None),
        Status::Ok => (1, None),
        Status::Error { description } => (2, Some(description.to_string())),
    }
}

fn time_to_nanos(time: SystemTime) -> String {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}

fn key_value<K: Into<String>>(key: K, value: &OtelValue) -> Value {
    let mut kv = Map::new();
    kv.insert("key".into(), Value::String(key.into()));
    kv.insert("value".into(), any_value(value));
    Value::Object(kv)
}

fn any_value(value: &OtelValue) -> Value {
    let mut inner = Map::new();
    match value {
        OtelValue::Bool(v) => {
            inner.insert("boolValue".into(), Value::Bool(*v));
        }
        OtelValue::I64(v) => {
            inner.insert("intValue".into(), Value::String(v.to_string()));
        }
        OtelValue::F64(v) => {
            if let Some(number) = Number::from_f64(*v) {
                inner.insert("doubleValue".into(), Value::Number(number));
            } else {
                inner.insert("stringValue".into(), Value::String(v.to_string()));
            }
        }
        OtelValue::String(v) => {
            inner.insert("stringValue".into(), Value::String(v.as_str().to_string()));
        }
        OtelValue::Array(array) => {
            let values = match array {
                opentelemetry::Array::Bool(items) => items
                    .iter()
                    .map(|v| any_value(&OtelValue::Bool(*v)))
                    .collect(),
                opentelemetry::Array::I64(items) => items
                    .iter()
                    .map(|v| any_value(&OtelValue::I64(*v)))
                    .collect(),
                opentelemetry::Array::F64(items) => items
                    .iter()
                    .map(|v| any_value(&OtelValue::F64(*v)))
                    .collect(),
                opentelemetry::Array::String(items) => items
                    .iter()
                    .map(|v| any_value(&OtelValue::String(v.clone())))
                    .collect(),
                #[allow(unreachable_patterns)]
                _ => Vec::new(),
            };
            let mut array_value = Map::new();
            array_value.insert("values".into(), Value::Array(values));
            inner.insert("arrayValue".into(), Value::Object(array_value));
        }
        #[allow(unreachable_patterns)]
        _ => {
            inner.insert("stringValue".into(), Value::String(format!("{value:?}")));
        }
    }

    Value::Object(inner)
}
