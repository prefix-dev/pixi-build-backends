use minijinja::Environment;
use serde::Serialize;

#[derive(Serialize)]
pub struct BuildScriptContext {
    pub build_platform: BuildPlatform,
    pub source_dir: String,
    pub extra_args: Vec<String>,
    /// The package has a host dependency on Python.
    /// This is used to determine if the build script
    /// should include Python-related logic.
    pub has_host_python: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildPlatform {
    Windows,
    Unix,
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
