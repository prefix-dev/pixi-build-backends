use std::sync::Arc;

use ordermap::OrderMap;
use pixi_build_types::{
    BinaryPackageSpecV1, PackageSpecV1, SourcePackageSpecV1, TargetV1, TargetsV1,
};
use rattler_conda_types::{Channel, MatchSpec, PackageName};
use recipe_stage0::{
    matchspec::{PackageDependency, SourceMatchSpec},
    recipe::{Conditional, ConditionalList, ConditionalRequirements, Item, ListOrItem},
    requirements::PackageSpecDependencies,
};
use url::Url;

use crate::encoded_source_spec_url::EncodedSourceSpecUrl;

pub fn from_source_url_to_source_package(source_url: Url) -> Option<SourcePackageSpecV1> {
    match source_url.scheme() {
        "source" => Some(EncodedSourceSpecUrl::from(source_url).into()),
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

pub(crate) fn source_package_spec_to_package_dependency(
    name: PackageName,
    source_spec: SourcePackageSpecV1,
) -> miette::Result<SourceMatchSpec> {
    let spec = MatchSpec {
        name: Some(name),
        ..Default::default()
    };

    Ok(SourceMatchSpec {
        spec,
        location: EncodedSourceSpecUrl::from(source_spec).into(),
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

    // If the version is "*", we treat it as None
    // so later rattler-build can detect the PackageDependency as a variant.
    let version = version.filter(|v| v != &rattler_conda_types::VersionSpec::Any);

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
    fn test_binary_package_conversion() {
        let name = PackageName::new_unchecked("foobar");
        let spec = BinaryPackageSpecV1 {
            version: Some("3.12.*".parse().unwrap()),
            ..BinaryPackageSpecV1::default()
        };
        let match_spec = binary_package_spec_to_package_dependency(name, spec);
        assert_eq!(match_spec.to_string(), "foobar 3.12.*");
    }

    #[test]
    fn test_binary_package_conversion_any_is_treated_as_none() {
        let name = PackageName::new_unchecked("python");
        let spec = BinaryPackageSpecV1 {
            version: Some("*".parse().unwrap()),
            ..BinaryPackageSpecV1::default()
        };
        let match_spec = binary_package_spec_to_package_dependency(name, spec);
        assert_eq!(match_spec.to_string(), "python");
    }
}
