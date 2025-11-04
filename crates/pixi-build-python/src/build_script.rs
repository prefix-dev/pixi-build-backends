use std::path::PathBuf;

use minijinja::Environment;
use rattler_conda_types::PackageName;
use recipe_stage0::{matchspec::PackageDependency, requirements::PackageSpecDependencies};
use serde::Serialize;

const UV: &str = "uv";
#[derive(Serialize)]
pub struct BuildScriptContext {
    pub installer: Installer,
    pub build_platform: BuildPlatform,
    pub editable: bool,
    pub extra_args: Vec<String>,
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

    pub fn determine_installer(
        dependencies: &PackageSpecDependencies<PackageDependency>,
    ) -> Installer {
        // Determine the installer to use
        let uv = PackageName::new_unchecked(UV.to_string());
        if dependencies.contains(&uv) {
            Installer::Uv
        } else {
            Installer::Pip
        }
    }

    /// Determine the installer from an iterator of dependency package names.
    /// Checks if "uv" is present in the package names.
    pub fn determine_installer_from_names<'a>(
        mut package_names: impl Iterator<Item = &'a str>,
    ) -> Installer
    {
        // Check all dependency names for "uv" package
        let has_uv = package_names.any(|name| name == UV);

        if has_uv {
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
    pub fn render(&self) -> String {
        let env = Environment::new();
        let template = env
            .template_from_str(include_str!("build_script.j2"))
            .unwrap();
        template.render(self).unwrap().trim().to_string()
    }
}
