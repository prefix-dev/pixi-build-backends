use std::path::PathBuf;

use minijinja::Environment;
use pixi_build_backend::{ProjectModel, Targets, traits::Dependencies};
use serde::Serialize;

const UV: &str = "uv";
#[derive(Serialize)]
pub struct BuildScriptContext {
    pub installer: Installer,
    pub build_platform: BuildPlatform,
    pub editable: bool,
    pub manifest_root: PathBuf,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Installer {
    Uv,
    #[default]
    Pip,
}

impl Installer {
    pub fn package_name(&self) -> &str {
        match self {
            Installer::Uv => "uv",
            Installer::Pip => "pip",
        }
    }

    pub fn determine_installer<P: ProjectModel>(
        dependencies: &Dependencies<<<P as ProjectModel>::Targets as Targets>::Spec>,
    ) -> Installer {
        // Determine the installer to use
        let uv = UV.to_string();
        if dependencies.host.contains_key(&uv)
            || dependencies.run.contains_key(&uv)
            || dependencies.build.contains_key(&uv)
        {
            Installer::Uv
        } else {
            Installer::Pip
        }
    }
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
