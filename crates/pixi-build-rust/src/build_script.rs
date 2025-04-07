use minijinja::Environment;
use serde::Serialize;

#[derive(Serialize)]
pub struct BuildScriptContext {
    /// The location of the source
    pub source_dir: String,

    /// Any additional args to pass to `cargo`
    pub extra_args: Vec<String>,

    /// True if `openssl` is part of the build environment
    pub has_openssl: bool,

    /// True if `sccache` is available.
    pub has_sccache: bool,

    /// The platform that is running the build.
    pub is_bash: bool,
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
