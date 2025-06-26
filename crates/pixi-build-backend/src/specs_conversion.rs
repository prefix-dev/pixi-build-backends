use std::str::FromStr;

use indexmap::IndexMap;
use miette::IntoDiagnostic;
use pixi_build_types::{PackageSpecV1, SourcePackageSpecV1, TargetV1, TargetsV1, UrlSpecV1};
use rattler_conda_types::{MatchSpec, PackageName};
use recipe_stage0::{
    matchspec::{PackageDependency, SourceMatchSpec},
    recipe::{Conditional, ConditionalList, ConditionalRequirements, Item, ListOrItem},
    requirements::PackageSpecDependencies,
};
use url::Url;

pub fn from_source_matchspec_into_package_spec(
    source_matchspec: SourceMatchSpec,
) -> SourcePackageSpecV1 {
    SourcePackageSpecV1::Url(UrlSpecV1 {
        url: source_matchspec.location,
        md5: None,
        sha256: None,
    })
}

pub fn from_targets_v1_to_conditional_requirements(targets: &TargetsV1) -> ConditionalRequirements {
    let mut build_items: ConditionalList<PackageDependency> = ConditionalList::new();
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

pub(crate) fn package_specs_to_package_dependency(
    specs: IndexMap<String, PackageSpecV1>,
) -> miette::Result<Vec<PackageDependency>> {
    specs
        .into_iter()
        .map(|(name, spec)| match spec {
            PackageSpecV1::Binary(_binary_spec) => Ok(PackageDependency::Binary(
                MatchSpec::from_str(name.as_str(), rattler_conda_types::ParseStrictness::Strict)
                    .unwrap(),
            )),

            PackageSpecV1::Source(source_spec) => {
                let name = PackageName::from_str(name.as_str()).into_diagnostic()?;

                let spec = MatchSpec {
                    name: Some(name.clone()),
                    ..Default::default()
                };
                let url_from_spec = match source_spec {
                    SourcePackageSpecV1::Path(path_spec) => {
                        Url::from_file_path(path_spec.path.clone()).unwrap()
                    }
                    _ => {
                        unimplemented!("Only URL source specs are supported for now")
                    }
                };

                Ok(PackageDependency::Source(SourceMatchSpec {
                    spec,
                    location: url_from_spec,
                }))
            }
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
