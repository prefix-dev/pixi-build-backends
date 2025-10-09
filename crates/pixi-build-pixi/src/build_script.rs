use serde::Serialize;

#[derive(Serialize)]
pub struct BuildScriptContext {
    pub build_task: String,
    pub manifest_root: std::path::PathBuf,
}

impl BuildScriptContext {
    pub fn render(&self) -> String {
        format!(
            "pixi run --as-is --manifest-path {} {}",
            self.manifest_root.to_string_lossy(),
            self.build_task
        )
    }
}
