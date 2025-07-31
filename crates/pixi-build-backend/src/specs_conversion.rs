use std::str::FromStr;

use miette::IntoDiagnostic;
use ordermap::OrderMap;
use pixi_build_types::{PackageSpecV1, SourcePackageSpecV1, TargetV1, TargetsV1};
use rattler_conda_types::{MatchSpec, PackageName};
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

pub(crate) fn package_specs_to_package_dependency(
    specs: OrderMap<String, PackageSpecV1>,
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
