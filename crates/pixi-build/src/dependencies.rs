use std::collections::BTreeMap;

use itertools::Either;
use miette::{Context, IntoDiagnostic};
use pixi_manifest::{CondaDependencies, Dependencies};
use pixi_spec::{PixiSpec, SourceSpec};
use rattler_build::{recipe::parser::Dependency, NormalizedKey};
use rattler_conda_types::{
    ChannelConfig, MatchSpec, NamelessMatchSpec, PackageName, ParseStrictness::Strict,
};

use crate::variants::can_be_used_as_variant;

/// A helper struct to extract match specs from a manifest.
pub struct MatchspecExtractor<'a> {
    channel_config: &'a ChannelConfig,
    variant: Option<&'a BTreeMap<NormalizedKey, String>>,
    ignore_self: bool,
}

impl<'a> MatchspecExtractor<'a> {
    pub fn new(channel_config: &'a ChannelConfig) -> Self {
        Self {
            channel_config,
            ignore_self: false,
            variant: None,
        }
    }

    /// If `ignore_self` is `true`, the conversion will skip dependencies that
    /// point to root directory itself.
    pub fn with_ignore_self(self, ignore_self: bool) -> Self {
        Self {
            ignore_self,
            ..self
        }
    }

    /// Sets the variant to use for the match specs.
    pub fn with_variant(self, variant: &'a BTreeMap<NormalizedKey, String>) -> Self {
        Self {
            variant: Some(variant),
            ..self
        }
    }

    /// Extracts match specs from the given set of dependencies.
    pub fn extract(&self, dependencies: CondaDependencies) -> miette::Result<Vec<MatchSpec>> {
        let root_dir = &self.channel_config.root_dir;
        let mut specs = Vec::new();
        for (name, spec) in dependencies.into_specs() {
            // If we have a variant override, we should use that instead of the spec.
            if can_be_used_as_variant(&spec) {
                if let Some(variant_value) = self
                    .variant
                    .as_ref()
                    .and_then(|variant| variant.get(&NormalizedKey::from(&name)))
                {
                    let spec = NamelessMatchSpec::from_str(variant_value, Strict)
                        .into_diagnostic()
                        .context("failed to convert variant to matchspec")?;
                    specs.push(MatchSpec::from_nameless(spec, Some(name)));
                    continue;
                }
            }

            let source_or_binary = spec.into_source_or_binary();
            let match_spec = match source_or_binary {
                Either::Left(SourceSpec::Path(path))
                    if self.ignore_self
                        && path
                            .resolve(root_dir)
                            .map_or(false, |path| path.as_path() == root_dir) =>
                {
                    // Skip source dependencies that point to the root directory. That would
                    // be a self reference.
                    continue;
                }
                Either::Left(_) => {
                    // All other source dependencies are not yet supported.
                    return Err(miette::miette!(
                        "recursive source dependencies are not yet supported"
                    ));
                }
                Either::Right(binary) => {
                    let nameless_spec = binary
                        .try_into_nameless_match_spec(self.channel_config)
                        .into_diagnostic()?;
                    if nameless_spec.version == Some("*".parse().unwrap()) {
                        // Skip dependencies with wildcard versions.
                        name.as_normalized().to_string().parse().unwrap()
                    } else {
                        MatchSpec::from_nameless(nameless_spec, Some(name))
                    }
                }
            };

            specs.push(match_spec);
        }

        Ok(specs)
    }
}

pub fn extract_dependencies(
    channel_config: &ChannelConfig,
    dependencies: Dependencies<PackageName, PixiSpec>,
    variant: &BTreeMap<NormalizedKey, String>,
) -> miette::Result<Vec<Dependency>> {
    Ok(MatchspecExtractor::new(channel_config)
        .with_ignore_self(true)
        .with_variant(variant)
        .extract(dependencies)?
        .into_iter()
        .map(Dependency::Spec)
        .collect())
}
