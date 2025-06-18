use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{
    build_script::{BuildPlatform, BuildScriptContext},
    config::CMakeBackendConfig,
};
use miette::IntoDiagnostic;
use pixi_build_backend::common::{PackageRequirements, SourceRequirements};
use pixi_build_backend::{ProjectModel, Targets};
use pixi_build_backend::{
    common::{BuildConfigurationParams, requirements},
    compilers::default_compiler,
    traits::{Dependencies, project::new_spec},
};
use rattler_build::metadata::Debug;
use rattler_build::recipe::parser::{BuildString, ScriptContent};
use rattler_build::{
    NormalizedKey,
    console_utils::LoggingOutputHandler,
    hash::HashInfo,
    metadata::{BuildConfiguration, PackagingSettings},
    recipe::{
        Recipe,
        parser::{Build, Dependency, Package, Script},
        variable::Variable,
    },
};
use rattler_conda_types::{MatchSpec, NoArchType, PackageName, Platform, package::ArchiveType};
use rattler_package_streaming::write::CompressionLevel;

pub struct CMakeBuildBackend<P: ProjectModel> {
    pub(crate) logging_output_handler: LoggingOutputHandler,
    pub(crate) manifest_path: PathBuf,
    pub(crate) manifest_root: PathBuf,
    pub(crate) project_model: P,
    pub(crate) config: CMakeBackendConfig,
    pub(crate) cache_dir: Option<PathBuf>,
}

impl<P: ProjectModel> CMakeBuildBackend<P> {
    /// Returns a new instance of [`CMakeBuildBackend`] by reading the manifest
    /// at the given path.
    pub fn new(
        manifest_path: &Path,
        project_model: P,
        config: CMakeBackendConfig,
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

    /// Returns the matchspecs for the compiler packages. That should be
    /// included in the build section of the recipe.
    fn compiler_packages(&self, target_platform: Platform) -> Vec<MatchSpec> {
        let mut compilers = vec![];

        for lang in self.languages() {
            if let Some(name) = default_compiler(target_platform, &lang) {
                // TODO: Read this from variants
                // TODO: Read the version specification from variants
                let compiler_package =
                    PackageName::new_unchecked(format!("{name}_{target_platform}"));
                compilers.push(MatchSpec::from(compiler_package));
            }

            // TODO: stdlib??
        }

        compilers
    }

    /// Returns the languages that are used in the cmake project. These define
    /// which compilers are required to build the project.
    fn languages(&self) -> Vec<String> {
        // TODO: Can we figure this out from looking at the CMake?
        vec!["cxx".to_string()]
    }

    /// Constructs a [`Recipe`] from the current manifest. The constructed
    /// recipe will invoke CMake to build and install the package.
    pub(crate) fn recipe(
        &self,
        host_platform: Platform,
        variant: &BTreeMap<NormalizedKey, Variable>,
    ) -> miette::Result<(Recipe, SourceRequirements<P>)> {
        // Parse the package name from the manifest
        let project_model = &self.project_model;
        let name = PackageName::from_str(project_model.name()).into_diagnostic()?;
        let version = self.project_model.version().clone().ok_or_else(|| {
            miette::miette!("a version is missing from the package but it is required")
        })?;

        let noarch_type = NoArchType::none();

        let requirements = self.requirements(host_platform, variant)?;

        let build_platform = Platform::current();
        let build_number = 0;

        let build_script = BuildScriptContext {
            build_platform: if build_platform.is_windows() {
                BuildPlatform::Windows
            } else {
                BuildPlatform::Unix
            },
            source_dir: self.manifest_root.display().to_string(),
            extra_args: self.config.extra_args.clone(),
        }
        .render();

        let hash_info = HashInfo::from_variant(variant, &noarch_type);

        Ok((
            Recipe {
                schema_version: 1,
                context: Default::default(),
                package: Package {
                    version: version.into(),
                    name,
                },
                cache: None,
                // source: vec![Source::Path(PathSource {
                //     // TODO: How can we use a git source?
                //     path: manifest_root.to_path_buf(),
                //     sha256: None,
                //     md5: None,
                //     patches: vec![],
                //     target_directory: None,
                //     file_name: None,
                //     use_gitignore: true,
                // })],
                // We hack the source location
                // as we dont want to copy the files
                source: vec![],
                build: Build {
                    number: build_number,
                    string: BuildString::Resolved(BuildString::compute(&hash_info, build_number)),

                    // skip: Default::default(),
                    script: Script {
                        content: ScriptContent::Commands(build_script),
                        env: self.config.env.clone(),
                        ..Default::default()
                    },
                    noarch: noarch_type,

                    // TODO: Python is not exposed properly
                    //python: Default::default(),
                    // dynamic_linking: Default::default(),
                    // always_copy_files: Default::default(),
                    // always_include_files: Default::default(),
                    // merge_build_and_host_envs: false,
                    // variant: Default::default(),
                    // prefix_detection: Default::default(),
                    // post_process: vec![],
                    // files: Default::default(),
                    ..Build::default()
                },
                // TODO read from manifest
                requirements: requirements.requirements,
                tests: vec![],
                about: Default::default(),
                extra: Default::default(),
            },
            requirements.source,
        ))
    }

    pub(crate) fn requirements(
        &self,
        host_platform: Platform,
        variant: &BTreeMap<NormalizedKey, Variable>,
    ) -> miette::Result<PackageRequirements<P>> {
        let project_model = &self.project_model;
        let dependencies = project_model.dependencies(Some(host_platform));

        let build_tools = build_tools();
        let empty_spec = new_spec::<P>();
        let dependencies = add_build_tools::<P>(dependencies, &build_tools, &empty_spec);

        let mut package_requirements = requirements::<P>(dependencies, variant)?;

        package_requirements.requirements.build.extend(
            self.compiler_packages(host_platform)
                .into_iter()
                .map(Dependency::Spec),
        );

        Ok(package_requirements)
    }
}

/// Returns the build tools that are required to build the project.
pub(crate) fn build_tools() -> Vec<String> {
    vec!["cmake".to_string(), "ninja".to_string()]
}

/// Adds the build tools to the dependencies.
pub(crate) fn add_build_tools<'a, P: ProjectModel>(
    mut dependencies: Dependencies<'a, <P::Targets as Targets>::Spec>,
    build_tools: &'a [String],
    empty_spec: &'a <<P as ProjectModel>::Targets as Targets>::Spec,
) -> Dependencies<'a, <<P as ProjectModel>::Targets as Targets>::Spec> {
    for pkg_name in build_tools.iter() {
        if dependencies.build.contains_key(pkg_name) {
            // If the host dependencies already contain the package, we don't need to add it
            // again.
            continue;
        }

        dependencies.build.insert(pkg_name, empty_spec);
    }

    dependencies
}

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
        debug: Debug::new(false),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use indexmap::IndexMap;
    use pixi_build_type_conversions::to_project_model_v1;
    use pixi_manifest::Manifests;
    use rattler_build::{console_utils::LoggingOutputHandler, recipe::Recipe};
    use rattler_conda_types::{ChannelConfig, Platform};
    use tempfile::tempdir;

    use crate::{cmake::CMakeBuildBackend, config::CMakeBackendConfig};

    fn recipe(manifest_source: &str, config: CMakeBackendConfig) -> Recipe {
        let tmp_dir = tempdir().unwrap();
        let tmp_manifest = tmp_dir.path().join("pixi.toml");
        std::fs::write(&tmp_manifest, manifest_source).unwrap();
        let manifest = Manifests::from_workspace_manifest_path(tmp_manifest.clone()).unwrap();
        let package = manifest.value.package.unwrap();
        let channel_config = ChannelConfig::default_with_root_dir(tmp_dir.path().to_path_buf());
        let project_model = to_project_model_v1(&package.value, &channel_config).unwrap();

        let cmake_backend = CMakeBuildBackend::new(
            &tmp_manifest,
            project_model,
            config,
            LoggingOutputHandler::default(),
            None,
        )
        .unwrap();

        cmake_backend
            .recipe(Platform::current(), &BTreeMap::new())
            .unwrap()
            .0
    }

    #[tokio::test]
    async fn test_setting_host_and_build_requirements() {
        // get cargo manifest dir

        let package_with_host_and_build_deps = r#"
        [workspace]
        name = "test-reqs"
        channels = ["conda-forge"]
        platforms = ["osx-arm64"]
        preview = ["pixi-build"]

        [package]
        name = "test-reqs"
        version = "1.0"

        [package.host-dependencies]
        hatchling = "*"

        [package.build-dependencies]
        boltons = "*"

        [package.run-dependencies]
        foobar = "==3.2.1"

        [package.build]
        backend = { name = "pixi-build-python", version = "*" }
        "#;

        let tmp_dir = tempdir().unwrap();
        let tmp_manifest = tmp_dir.path().join("pixi.toml");

        // write the raw string into the file
        std::fs::write(&tmp_manifest, package_with_host_and_build_deps).unwrap();

        let manifest = Manifests::from_workspace_manifest_path(tmp_manifest).unwrap();
        let package = manifest.value.package.unwrap();
        let channel_config = ChannelConfig::default_with_root_dir(tmp_dir.path().to_path_buf());
        let project_model = to_project_model_v1(&package.value, &channel_config).unwrap();
        let cmake_backend = CMakeBuildBackend::new(
            &package.provenance.path,
            project_model,
            CMakeBackendConfig::default(),
            LoggingOutputHandler::default(),
            None,
        )
        .unwrap();

        let host_platform = Platform::current();

        let (recipe, _source_requirements) = cmake_backend
            .recipe(host_platform, &BTreeMap::new())
            .unwrap();
        insta::with_settings!({
            filters => vec![
                ("(vs2017|vs2019|gxx|clang).*", "\"[ ... compiler ... ]\""),
            ]
        }, {
            insta::assert_yaml_snapshot!(recipe, {
               ".build.script" => "[ ... script ... ]",
            });
        });
    }

    #[tokio::test]
    async fn test_parsing_subdirectory() {
        // a manifest with subdir

        let package_with_git_and_subdir = r#"
        [workspace]
        name = "test-reqs"
        channels = ["conda-forge"]
        platforms = ["osx-arm64"]
        preview = ["pixi-build"]

        [package]
        name = "test-reqs"
        version = "1.0"

        [package.build]
        backend = { name = "pixi-build-python", version = "*" }

        [package.host-dependencies]
        hatchling = { git = "git+https://github.com/hatchling/hatchling.git", subdirectory = "src" }
        "#;

        let tmp_dir = tempdir().unwrap();
        let tmp_manifest = tmp_dir.path().join("pixi.toml");

        // write the raw string into the file
        std::fs::write(&tmp_manifest, package_with_git_and_subdir).unwrap();

        Manifests::from_workspace_manifest_path(tmp_manifest).unwrap();
    }

    #[test]
    fn test_env_vars_are_set() {
        let manifest_source = r#"
        [workspace]
        platforms = []
        channels = []
        preview = ["pixi-build"]

        [package]
        name = "foobar"
        version = "0.1.0"

        [package.build]
        backend = { name = "pixi-build-rust", version = "*" }
        "#;

        let env = IndexMap::from([("foo".to_string(), "bar".to_string())]);

        let recipe = recipe(
            manifest_source,
            CMakeBackendConfig {
                env: env.clone(),
                ..Default::default()
            },
        );

        assert_eq!(recipe.build.script.env, env);
    }
}
