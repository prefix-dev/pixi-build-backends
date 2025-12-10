use crate::config::BuildTask;
use serde::Serialize;

#[derive(Serialize)]
pub struct BuildScriptContext {
    pub build_tasks: Vec<BuildTask>,
    pub manifest_root: std::path::PathBuf,
}

impl BuildScriptContext {
    pub fn render(&self) -> String {
        self.build_tasks
            .iter()
            .map(|build_task| {
                let env_arg = build_task
                    .environment
                    .as_ref()
                    .map(|env| format!(" -e {}", env))
                    .unwrap_or_default();
                format!(
                    "pixi run --as-is --manifest-path {}{}  {}",
                    self.manifest_root.to_string_lossy(),
                    env_arg,
                    build_task.task
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
