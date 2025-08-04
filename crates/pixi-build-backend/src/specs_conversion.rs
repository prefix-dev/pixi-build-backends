use std::{path::Path, str::FromStr, sync::Arc};

use ordermap::OrderMap;
use pixi_build_types::{
    BinaryPackageSpecV1, GitSpecV1, PackageSpecV1, SourcePackageSpecV1, TargetV1, TargetsV1,
    UrlSpecV1,
};
use rattler_conda_types::{Channel, MatchSpec, PackageName};
use recipe_stage0::{
    matchspec::{PackageDependency, SourceMatchSpec},
    recipe::{Conditional, ConditionalList, ConditionalRequirements, Item, ListOrItem},
    requirements::PackageSpecDependencies,
};
use url::Url;

pub fn from_source_url_to_source_package(source_url: Url) -> Option<SourcePackageSpecV1> {
    match source_url.scheme() {
        "source" => Some(SourcePackageSpecV1::Path(pixi_build_types::PathSpecV1 {
            path: SafeRelativePathUrl::from(source_url).to_path(),
        })),
        "http" | "https" => {
            // For now, we only support URL sources with no checksums.
            Some(SourcePackageSpecV1::Url(UrlSpecV1 {
                url: source_url,
                md5: None,
                sha256: None,
            }))
        }
        "git" => {
            // For git URLs, we can only support the URL without any additional metadata.
            // This is a limitation of the current implementation.
            Some(SourcePackageSpecV1::Git(GitSpecV1 {
                git: source_url,
                rev: None,
                subdirectory: None,
            }))
        }
        _ => None,
    }
}

pub fn from_source_matchspec_into_package_spec(
    source_matchspec: SourceMatchSpec,
) -> miette::Result<SourcePackageSpecV1> {
    from_source_url_to_source_package(source_matchspec.location)
        .ok_or_else(|| miette::miette!("Only file, http/https and git are supported for now"))
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

fn binary_package_spec_to_package_dependency(
    name: PackageName,
    binary_spec: BinaryPackageSpecV1,
) -> PackageDependency {
    let BinaryPackageSpecV1 {
        version,
        build,
        build_number,
        file_name,
        channel,
        subdir,
        md5,
        sha256,
        url,
        license,
    } = binary_spec;

    PackageDependency::Binary(MatchSpec {
        name: Some(name),
        version,
        build,
        build_number,
        file_name,
        extras: None,
        channel: channel.map(Channel::from_url).map(Arc::new),
        subdir,
        namespace: None,
        md5,
        sha256,
        url,
        license,
    })
}

fn package_spec_to_package_dependency(
    name: PackageName,
    spec: PackageSpecV1,
) -> miette::Result<PackageDependency> {
    match spec {
        PackageSpecV1::Binary(binary_spec) => Ok(binary_package_spec_to_package_dependency(
            name,
            *binary_spec,
        )),
        PackageSpecV1::Source(source_spec) => Ok(PackageDependency::Source(
            source_package_spec_to_package_dependency(name, source_spec)?,
        )),
    }
}

pub(crate) fn package_specs_to_package_dependency(
    specs: OrderMap<String, PackageSpecV1>,
) -> miette::Result<Vec<PackageDependency>> {
    specs
        .into_iter()
        .map(|(name, spec)| {
            package_spec_to_package_dependency(PackageName::new_unchecked(name), spec)
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

    #[test]
    fn test_binary_package_conversion() {
        let name = PackageName::new_unchecked("foobar");
        let spec = BinaryPackageSpecV1 {
            version: Some("3.12.*".parse().unwrap()),
            ..BinaryPackageSpecV1::default()
        };
        let match_spec = binary_package_spec_to_package_dependency(name, spec);
        assert_eq!(match_spec.to_string(), "foobar 3.12.*");
    }
}
