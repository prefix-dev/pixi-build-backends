use minijinja::Environment;
use serde::Serialize;

#[derive(Serialize)]
pub struct BuildScriptContext {
    pub source_dir: String,
    pub extra_args: Vec<String>,
    pub export_openssl: bool,
    pub has_sccache: bool,
}

impl BuildScriptContext {
    pub fn render(&self) -> Vec<String> {
        let env = Environment::new();
        let template = env
            .template_from_str(include_str!("build_script.j2"))
            .unwrap();
        let rendered = template.render(self).unwrap().to_string();
        rendered.lines().map(|s| s.to_string()).collect()
    }
}
