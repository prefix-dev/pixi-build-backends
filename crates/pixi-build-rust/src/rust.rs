use std::{collections::BTreeMap, path::PathBuf, str::FromStr};

use miette::IntoDiagnostic;
use pixi_build_backend::{
    common::{requirements, BuildConfigurationParams},
    compilers::default_compiler,
    ProjectModel,
};
use rattler_build::{
    console_utils::LoggingOutputHandler,
    hash::HashInfo,
    metadata::{BuildConfiguration, PackagingSettings},
    recipe::{
        parser::{Build, Dependency, Package, Requirements, ScriptContent},
        variable::Variable,
        Recipe,
    },
    NormalizedKey,
};
use rattler_conda_types::{
    package::ArchiveType, ChannelConfig, MatchSpec, NoArchType, PackageName, Platform,
};
use rattler_package_streaming::write::CompressionLevel;

use crate::{build_script::BuildScriptContext, config::RustBackendConfig};

pub struct RustBuildBackend<P: ProjectModel> {
    pub(crate) logging_output_handler: LoggingOutputHandler,
    pub(crate) manifest_path: PathBuf,
    pub(crate) manifest_root: PathBuf,
    pub(crate) project_model: P,
    pub(crate) config: RustBackendConfig,
    pub(crate) cache_dir: Option<PathBuf>,
}

impl<P: ProjectModel> RustBuildBackend<P> {
    /// Returns a new instance of [`RustBuildBackend`].
    pub fn new(
        manifest_path: PathBuf,
        project_model: P,
        config: RustBackendConfig,
        logging_output_handler: LoggingOutputHandler,
        cache_dir: Option<PathBuf>,
    ) -> miette::Result<Self> {
        // Determine the root directory of the manifest
        let manifest_root = manifest_path
            .parent()
            .ok_or_else(|| miette::miette!("the project manifest must reside in a directory"))?
            .to_path_buf();

        Ok(Self {
            manifest_path: manifest_path.to_path_buf(),
            manifest_root,
            project_model,
            config,
            logging_output_handler,
            cache_dir,
        })
    }

    /// Returns the matchspecs for the compiler packages.
    /// That should be included in the build section of the recipe.
    /// TODO: Should we also take into account other compilers like
    /// c or cxx?
    fn compiler_packages(&self, target_platform: Platform) -> Vec<MatchSpec> {
        let mut compilers = vec![];

        if let Some(name) = default_compiler(target_platform, "rust") {
            // TODO: Read this from variants
            // TODO: Read the version specification from variants
            let compiler_package = PackageName::new_unchecked(format!("{name}_{target_platform}"));
            compilers.push(MatchSpec::from(compiler_package));
        }

        compilers
    }

    /// Constructs a [`Recipe`] that will build the python package into a conda
    /// package.
    pub(crate) fn recipe(
        &self,
        host_platform: Platform,
        channel_config: &ChannelConfig,
        variant: &BTreeMap<NormalizedKey, Variable>,
    ) -> miette::Result<Recipe> {
        // Parse the package name and version from the manifest
        let name = PackageName::from_str(self.project_model.name()).into_diagnostic()?;
        let version = self.project_model.version().clone().ok_or_else(|| {
            miette::miette!("a version is missing from the package but it is required")
        })?;

        let noarch_type = NoArchType::none();

        let requirements = self.requirements(host_platform, channel_config, variant)?;

        let build_number = 0;

        let build_script = BuildScriptContext {
            source_dir: self.manifest_root.display().to_string(),
            extra_args: self.config.extra_args.clone(),
        }
        .render();

        Ok(Recipe {
            schema_version: 1,
            package: Package {
                version: version.into(),
                name,
            },
            context: Default::default(),
            cache: None,
            // Sometimes rust projects could be a part of a workspace, so we need to
            // include entire source project
            // and set the source directory to the root of the package.
            source: vec![],
            build: Build {
                number: build_number,
                string: Default::default(),
                script: ScriptContent::Commands(build_script).into(),
                noarch: noarch_type,
                ..Build::default()
            },
            requirements,
            tests: vec![],
            about: Default::default(),
            extra: Default::default(),
        })
    }

    pub(crate) fn requirements(
        &self,
        host_platform: Platform,
        channel_config: &ChannelConfig,
        variant: &BTreeMap<NormalizedKey, Variable>,
    ) -> miette::Result<Requirements> {
        let project_model = &self.project_model;
        let dependencies = project_model.dependencies(Some(host_platform));

        let mut requirements = requirements::<P>(dependencies, channel_config, variant)?;

        requirements.build.extend(
            self.compiler_packages(host_platform)
                .into_iter()
                .map(Dependency::Spec),
        );

        Ok(requirements)
    }
}

/// Construct a build configuration for the given recipe and parameters.
pub(crate) fn construct_configuration(
    recipe: &Recipe,
    params: BuildConfigurationParams,
) -> BuildConfiguration {
    BuildConfiguration {
        target_platform: params.host_platform.platform,
        host_platform: params.host_platform,
        build_platform: params.build_platform,
        hash: HashInfo::from_variant(&params.variant, &recipe.build.noarch),
        variant: params.variant,
        directories: params.directories,
        channels: params.channels,
        channel_priority: Default::default(),
        solve_strategy: Default::default(),
        timestamp: chrono::Utc::now(),
        subpackages: Default::default(), // TODO: ???
        packaging_settings: PackagingSettings::from_args(
            ArchiveType::Conda,
            CompressionLevel::default(),
        ),
        store_recipe: false,
        force_colors: true,
        sandbox_config: None,
    }
}

#[cfg(test)]
mod tests {

    use std::collections::BTreeMap;

    use pixi_build_type_conversions::to_project_model_v1;

    use pixi_manifest::Manifests;
    use rattler_build::{console_utils::LoggingOutputHandler, recipe::Recipe};
    use rattler_conda_types::{ChannelConfig, Platform};
    use tempfile::tempdir;

    use crate::{config::RustBackendConfig, rust::RustBuildBackend};

    fn recipe(manifest_source: &str, config: RustBackendConfig) -> Recipe {
        let tmp_dir = tempdir().unwrap();
        let tmp_manifest = tmp_dir.path().join("pixi.toml");
        std::fs::write(&tmp_manifest, manifest_source).unwrap();
        let manifest = Manifests::from_workspace_manifest_path(tmp_manifest.clone()).unwrap();
        let package = manifest.value.package.unwrap();
        let channel_config = ChannelConfig::default_with_root_dir(tmp_dir.path().to_path_buf());
        let project_model = to_project_model_v1(&package.value, &channel_config).unwrap();

        let python_backend = RustBuildBackend::new(
            tmp_manifest,
            project_model,
            config,
            LoggingOutputHandler::default(),
            None,
        )
        .unwrap();

        python_backend
            .recipe(Platform::current(), &channel_config, &BTreeMap::new())
            .unwrap()
    }

    #[test]
    fn test_rust_is_in_build_requirements() {
        insta::assert_yaml_snapshot!(recipe(r#"
        [workspace]
        platforms = []
        channels = []
        preview = ["pixi-build"]

        [package]
        name = "foobar"
        version = "0.1.0"

        [package.build]
        backend = { name = "pixi-build-rust", version = "*" }
        "#, RustBackendConfig::default()), {
        ".source[0].path" => "[ ... path ... ]",
        ".build.script" => "[ ... script ... ]",
        ".requirements.build[0]" => insta::dynamic_redaction(|value, _path| {
            // assert that the value looks like a uuid here
            assert!(value.as_str().unwrap().contains("rust"));
            }),
        });
    }
}
