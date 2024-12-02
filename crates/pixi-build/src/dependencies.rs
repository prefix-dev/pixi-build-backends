use itertools::Either;
use miette::IntoDiagnostic;
use pixi_manifest::{CondaDependencies, Dependencies};
use pixi_spec::{PixiSpec, SourceSpec};
use rattler_build::recipe::parser::Dependency;
use rattler_conda_types::{ChannelConfig, MatchSpec, PackageName};

/// A helper struct to extract match specs from a manifest.
pub struct MatchspecExtractor<'a> {
    channel_config: &'a ChannelConfig,
    ignore_self: bool,
}

impl<'a> MatchspecExtractor<'a> {
    pub fn new(channel_config: &'a ChannelConfig) -> Self {
        Self {
            channel_config,
            ignore_self: false,
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

    /// Extracts match specs from the given set of dependencies.
    pub fn extract(&self, dependencies: CondaDependencies) -> miette::Result<Vec<MatchSpec>> {
        let root_dir = &self.channel_config.root_dir;
        let mut specs = Vec::new();
        for (name, spec) in dependencies.into_specs() {
            let source_or_binary = spec.into_source_or_binary();
            let match_spec = match source_or_binary {
                Either::Left(SourceSpec::Path(path))
                    if self.ignore_self
                        && path
                            .resolve(root_dir)
                            .map_or(false, |path| path.as_path() == root_dir) =>
                {
                    // Skip source dependencies that point to the root directory. That would
                    // be a self reference.
                    continue;
                }
                Either::Left(_) => {
                    // All other source dependencies are not yet supported.
                    return Err(miette::miette!(
                        "recursive source dependencies are not yet supported"
                    ));
                }
                Either::Right(binary) => {
                    let nameless_spec = binary
                        .try_into_nameless_match_spec(self.channel_config)
                        .into_diagnostic()?;
                    MatchSpec::from_nameless(nameless_spec, Some(name))
                }
            };

            specs.push(match_spec);
        }

        Ok(specs)
    }
}

pub fn extract_dependencies(
    channel_config: &ChannelConfig,
    dependencies: Dependencies<PackageName, PixiSpec>,
) -> miette::Result<Vec<Dependency>> {
    Ok(MatchspecExtractor::new(channel_config)
        .with_ignore_self(true)
        .extract(dependencies)?
        .into_iter()
        .map(Dependency::Spec)
        .collect())
}
