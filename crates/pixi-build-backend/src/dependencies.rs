use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    str::FromStr,
};

use miette::{Context, IntoDiagnostic};
use pixi_build_types as pbt;
use rattler_build::{
    recipe::{
        parser::{Dependency, Requirements},
        variable::Variable,
    },
    NormalizedKey,
};
use rattler_conda_types::{
    ChannelConfig, MatchSpec, NamelessMatchSpec, PackageName, ParseStrictness::Strict,
};

use crate::{
    traits::{Dependencies, PackageSpec},
    Targets,
};

/// A helper struct to extract match specs from a manifest.
pub struct MatchspecExtractor<'a> {
    channel_config: &'a ChannelConfig,
    variant: Option<&'a BTreeMap<NormalizedKey, Variable>>,
    ignore_self: bool,
}

/// Resolves the path relative to `root_dir`. If the path is absolute,
/// it is returned verbatim.
///
/// May return an error if the path is prefixed with `~` and the home
/// directory is undefined.
pub(crate) fn resolve_path(path: &Path, root_dir: impl AsRef<Path>) -> Option<PathBuf> {
    if path.is_absolute() {
        Some(PathBuf::from(path))
    } else if let Ok(user_path) = path.strip_prefix("~/") {
        dirs::home_dir().map(|h| h.join(user_path))
    } else {
        Some(root_dir.as_ref().join(path))
    }
}

impl<'a> MatchspecExtractor<'a> {
    pub fn new(channel_config: &'a ChannelConfig) -> Self {
        Self {
            channel_config,
            ignore_self: false,
            variant: None,
        }
    }

    /// If `ignore_self` is `true`, the conversion will skip dependencies that
    /// point to root directory itself.
    pub fn with_ignore_self(self, ignore_self: bool) -> Self {
        Self {
            ignore_self,
            ..self
        }
    }

    /// Sets the variant to use for the match specs.
    pub fn with_variant(self, variant: &'a BTreeMap<NormalizedKey, Variable>) -> Self {
        Self {
            variant: Some(variant),
            ..self
        }
    }

    /// Extracts match specs from the given set of dependencies.
    pub fn extract<'b, S>(
        &self,
        dependencies: impl IntoIterator<Item = (&'b pbt::SourcePackageName, &'b S)>,
    ) -> miette::Result<Vec<MatchSpec>>
    where
        S: PackageSpec + 'b,
    {
        let root_dir = &self.channel_config.root_dir;
        let mut specs = Vec::new();
        for (name, spec) in dependencies.into_iter() {
            let name = PackageName::from_str(name.as_str()).into_diagnostic()?;
            // If we have a variant override, we should use that instead of the spec.
            if spec.can_be_used_as_variant() {
                if let Some(variant_value) = self
                    .variant
                    .as_ref()
                    .and_then(|variant| variant.get(&NormalizedKey::from(&name)))
                {
                    let spec = NamelessMatchSpec::from_str(
                        variant_value.as_ref().as_str().wrap_err_with(|| {
                            miette::miette!("Variant {variant_value} needs to be a string")
                        })?,
                        Strict,
                    )
                    .into_diagnostic()
                    .context("failed to convert variant to matchspec")?;
                    specs.push(MatchSpec::from_nameless(spec, Some(name)));
                    continue;
                }
            }

            // Match on supported packages
            let match_spec = spec.to_match_spec(name, root_dir, self.ignore_self)?;

            specs.push(match_spec);
        }

        Ok(specs)
    }
}

pub fn extract_dependencies<'a, T>(
    channel_config: &ChannelConfig,
    dependencies: impl IntoIterator<Item = (&'a pbt::SourcePackageName, &'a T)>,
    variant: &BTreeMap<NormalizedKey, Variable>,
) -> miette::Result<Vec<Dependency>>
where
    T: PackageSpec + 'a,
{
    Ok(MatchspecExtractor::new(channel_config)
        .with_ignore_self(true)
        .with_variant(variant)
        .extract(dependencies)?
        .into_iter()
        .map(Dependency::Spec)
        .collect())
}

/// Extracts and processes dependencies for different sections of package requirements
pub fn process_requirements<F, T>(
    _project_model: &T,
    empty_spec: &<T::Targets as Targets>::Spec,
    dependencies: &mut Dependencies<<T::Targets as Targets>::Spec>,
    channel_config: &ChannelConfig,
    variant: &BTreeMap<NormalizedKey, Variable>,
    build_tools: &[&str],
    mut build_tools_processor: F,
) -> miette::Result<Requirements>
where
    F: FnMut(
        &mut Dependencies<<T::Targets as Targets>::Spec>,
        &<T::Targets as Targets>::Spec,
        &[&str],
    ),
    T: crate::ProjectModel,
{
    let mut requirements = Requirements::default();

    // Apply build tools processor to modify dependencies as needed
    // let empty_spec = project_model.new_spec();
    build_tools_processor(dependencies, empty_spec, build_tools);

    // Extract dependencies into requirements
    requirements.build = extract_dependencies(channel_config, dependencies.build.clone(), variant)?;
    requirements.host = extract_dependencies(channel_config, dependencies.host.clone(), variant)?;
    requirements.run = extract_dependencies(channel_config, dependencies.run.clone(), variant)?;

    Ok(requirements)
}
