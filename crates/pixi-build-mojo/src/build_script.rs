use super::config::{MojoBinConfig, MojoPkgConfig};
use minijinja::Environment;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct BuildScriptContext {
    /// The directory where the source code is located, the manifest root.
    pub source_dir: String,
    /// The directory name to place output artifacts, will be created in `source_dir`.
    pub dist: Option<String>,
    /// Any executable artifacts to create.
    pub bins: Option<Vec<MojoBinConfig>>,
    /// Any packages to create.
    pub pkg: Option<MojoPkgConfig>,

    /// Not currenlty used
    /// The package has a host dependency on Python.
    /// This is used to determine if the build script
    /// should include Python-related logic.
    pub has_host_python: bool,
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
