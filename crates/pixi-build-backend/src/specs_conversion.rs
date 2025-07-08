use std::{path::Path, str::FromStr};

use indexmap::IndexMap;
use miette::IntoDiagnostic;
use pixi_build_types::{
    GitSpecV1, PackageSpecV1, SourcePackageSpecV1, TargetV1, TargetsV1, UrlSpecV1,
};
use rattler_conda_types::{MatchSpec, PackageName};
use recipe_stage0::{
    matchspec::{PackageDependency, SourceMatchSpec},
    recipe::{Conditional, ConditionalList, ConditionalRequirements, Item, ListOrItem},
    requirements::PackageSpecDependencies,
};
use url::Url;

pub fn from_source_matchspec_into_package_spec(
    source_matchspec: SourceMatchSpec,
) -> miette::Result<SourcePackageSpecV1> {
    let source_url = source_matchspec.location;
    match source_url.scheme() {
        "source" => Ok(SourcePackageSpecV1::Path(pixi_build_types::PathSpecV1 {
            path: SafeRelativePathUrl::from(source_url).to_path(),
        })),
        "http" | "https" => {
            // For now, we only support URL sources with no checksums.
            Ok(SourcePackageSpecV1::Url(UrlSpecV1 {
                url: source_url,
                md5: None,
                sha256: None,
            }))
        }
        "git" => {
            // For git URLs, we can only support the URL without any additional metadata.
            // This is a limitation of the current implementation.
            Ok(SourcePackageSpecV1::Git(GitSpecV1 {
                git: source_url,
                rev: None,
                subdirectory: None,
            }))
        }
        _ => unimplemented!("Only file, http/https and git are supported for now"),
    }
}

pub fn from_targets_v1_to_conditional_requirements(targets: &TargetsV1) -> ConditionalRequirements {
    let mut build_items = ConditionalList::new();
    let mut host_items = ConditionalList::new();
    let mut run_items = ConditionalList::new();
    let run_constraints_items = ConditionalList::new();

    // Add default target
    if let Some(default_target) = &targets.default_target {
        let package_requirements = target_to_package_spec(default_target);

        // source_target_requirements.default_target = source_requirements;

        build_items.extend(
            package_requirements
                .build
                .into_iter()
                .map(|spec| spec.1)
                .map(Item::from),
        );

        host_items.extend(
            package_requirements
                .host
                .into_iter()
                .map(|spec| spec.1)
                .map(Item::from),
        );

        run_items.extend(
            package_requirements
                .run
                .into_iter()
                .map(|spec| spec.1)
                .map(Item::from),
        );
    }

    // Add specific targets
    if let Some(specific_targets) = &targets.targets {
        for (selector, target) in specific_targets {
            let package_requirements = target_to_package_spec(target);

            // add the binary requirements
            build_items.extend(
                package_requirements
                    .build
                    .into_iter()
                    .map(|spec| spec.1)
                    .map(|spec| {
                        Conditional {
                            condition: selector.to_string(),
                            then: ListOrItem(vec![spec]),
                            else_value: ListOrItem::default(),
                        }
                        .into()
                    }),
            );
            host_items.extend(
                package_requirements
                    .host
                    .into_iter()
                    .map(|spec| spec.1)
                    .map(|spec| {
                        Conditional {
                            condition: selector.to_string(),
                            then: ListOrItem(vec![spec]),
                            else_value: ListOrItem::default(),
                        }
                        .into()
                    }),
            );
            run_items.extend(
                package_requirements
                    .run
                    .into_iter()
                    .map(|spec| spec.1)
                    .map(|spec| {
                        Conditional {
                            condition: selector.to_string(),
                            then: ListOrItem(vec![spec]),
                            else_value: ListOrItem::default(),
                        }
                        .into()
                    }),
            );
        }
    }

    ConditionalRequirements {
        build: build_items,
        host: host_items,
        run: run_items,
        run_constraints: run_constraints_items,
    }
}

/// An internal type that supports converting a path (and relative paths) into a
/// valid URL and back.
struct SafeRelativePathUrl(Url);

impl From<SafeRelativePathUrl> for Url {
    fn from(value: SafeRelativePathUrl) -> Self {
        value.0
    }
}

impl From<Url> for SafeRelativePathUrl {
    fn from(url: Url) -> Self {
        // Ensure the URL is a file URL
        assert_eq!(url.scheme(), "source", "URL must be a file URL");
        Self(url)
    }
}

impl SafeRelativePathUrl {
    pub fn from_path(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        Self(
            Url::from_str(&format!("source://?path={}", path.to_string_lossy()))
                .expect("must be a valid URL now"),
        )
    }

    pub fn to_path(&self) -> String {
        self.0
            .query_pairs()
            .find_map(|(key, value)| (key == "path").then_some(value))
            .expect("must have a path")
            .into_owned()
    }
}

pub(crate) fn source_package_spec_to_package_dependency(
    name: PackageName,
    source_spec: SourcePackageSpecV1,
) -> miette::Result<SourceMatchSpec> {
    let spec = MatchSpec {
        name: Some(name),
        ..Default::default()
    };

    let url_from_spec = match source_spec {
        SourcePackageSpecV1::Path(path_spec) => {
            SafeRelativePathUrl::from_path(Path::new(&path_spec.path)).into()
        }
        SourcePackageSpecV1::Url(url_spec) => url_spec.url,
        SourcePackageSpecV1::Git(git_spec) => git_spec.git,
    };

    Ok(SourceMatchSpec {
        spec,
        location: url_from_spec,
    })
}

pub(crate) fn package_specs_to_package_dependency(
    specs: IndexMap<String, PackageSpecV1>,
) -> miette::Result<Vec<PackageDependency>> {
    specs
        .into_iter()
        .map(|(name, spec)| match spec {
            PackageSpecV1::Binary(_binary_spec) => Ok(PackageDependency::Binary(
                MatchSpec::from_str(name.as_str(), rattler_conda_types::ParseStrictness::Strict)
                    .into_diagnostic()?,
            )),

            PackageSpecV1::Source(source_spec) => Ok(PackageDependency::Source(
                source_package_spec_to_package_dependency(
                    PackageName::from_str(&name).into_diagnostic()?,
                    source_spec,
                )?,
            )),
        })
        .collect()
}

// TODO: Should it be a From implementation?
pub fn target_to_package_spec(target: &TargetV1) -> PackageSpecDependencies<PackageDependency> {
    let build_reqs = target
        .clone()
        .build_dependencies
        .map(|deps| package_specs_to_package_dependency(deps).unwrap())
        .unwrap_or_default();

    let host_reqs = target
        .clone()
        .host_dependencies
        .map(|deps| package_specs_to_package_dependency(deps).unwrap())
        .unwrap_or_default();

    let run_reqs = target
        .clone()
        .run_dependencies
        .map(|deps| package_specs_to_package_dependency(deps).unwrap())
        .unwrap_or_default();

    let mut bin_reqs = PackageSpecDependencies::default();

    for spec in build_reqs.iter() {
        bin_reqs.build.insert(spec.package_name(), spec.clone());
    }

    for spec in host_reqs.iter() {
        bin_reqs.host.insert(spec.package_name(), spec.clone());
    }

    for spec in run_reqs.iter() {
        bin_reqs.run.insert(spec.package_name(), spec.clone());
    }

    bin_reqs
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_safe_relative_path_url() {
        let url = SafeRelativePathUrl::from_path("..\\test\\path");
        assert_eq!(url.to_path(), "..\\test\\path");

        // Retains original slashes
        let url = SafeRelativePathUrl::from_path("../test/path");
        assert_eq!(url.to_path(), "../test/path");

        let url = SafeRelativePathUrl::from_path("test/path");
        assert_eq!(url.to_path(), "test/path");

        let url = SafeRelativePathUrl::from_path("/absolute/test/path");
        assert_eq!(url.to_path(), "/absolute/test/path");
    }
}
