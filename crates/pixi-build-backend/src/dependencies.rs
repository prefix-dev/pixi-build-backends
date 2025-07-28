use std::{
    collections::{BTreeMap, HashMap},
    str::FromStr,
};

use miette::{Context, Diagnostic, IntoDiagnostic};
use pixi_build_types as pbt;
use pixi_build_types::{BinaryPackageSpecV1, NamedSpecV1};
use rattler_build::{
    NormalizedKey,
    metadata::PackageIdentifier,
    recipe::{parser::Dependency, variable::Variable},
    render::{
        pin::PinError,
        resolved_dependencies::{
            DependencyInfo, PinCompatibleDependency, PinSubpackageDependency, ResolveError,
            SourceDependency, VariantDependency,
        },
    },
};
use rattler_conda_types::{
    MatchSpec, NamelessMatchSpec, PackageName, PackageRecord, ParseStrictness::Strict,
};
use thiserror::Error;

use crate::{specs_conversion::from_source_url_to_source_package, traits::PackageSpec};

/// A helper struct to extract match specs from a manifest.
#[derive(Default)]
pub struct MatchspecExtractor<'a> {
    variant: Option<&'a BTreeMap<NormalizedKey, Variable>>,
}

pub struct ExtractedMatchSpecs<S: PackageSpec> {
    pub specs: Vec<MatchSpec>,
    pub sources: HashMap<String, S::SourceSpec>,
}

impl<'a> MatchspecExtractor<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the variant to use for the match specs.
    pub fn with_variant(self, variant: &'a BTreeMap<NormalizedKey, Variable>) -> Self {
        Self {
            variant: Some(variant),
        }
    }

    /// Extracts match specs from the given set of dependencies.
    pub fn extract<'b, S>(
        &self,
        dependencies: impl IntoIterator<Item = (&'b pbt::SourcePackageName, &'b S)>,
    ) -> miette::Result<ExtractedMatchSpecs<S>>
    where
        S: PackageSpec + 'b,
    {
        let mut specs = Vec::new();
        let mut source_specs = HashMap::new();
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
            let (match_spec, source_spec) = spec.to_match_spec(name.clone())?;

            specs.push(match_spec);
            if let Some(source_spec) = source_spec {
                source_specs.insert(name.as_normalized().to_owned(), source_spec);
            }
        }

        Ok(ExtractedMatchSpecs {
            specs,
            sources: source_specs,
        })
    }
}

pub struct ExtractedDependencies<T: PackageSpec> {
    pub dependencies: Vec<Dependency>,
    pub sources: HashMap<String, T::SourceSpec>,
}

impl<T: PackageSpec> ExtractedDependencies<T> {
    pub fn from_dependencies<'a>(
        dependencies: impl IntoIterator<Item = (&'a pbt::SourcePackageName, &'a T)>,
        variant: &BTreeMap<NormalizedKey, Variable>,
    ) -> miette::Result<Self>
    where
        T: 'a,
    {
        let extracted_specs = MatchspecExtractor::new()
            .with_variant(variant)
            .extract(dependencies)?;

        Ok(Self {
            dependencies: extracted_specs
                .specs
                .into_iter()
                .map(Dependency::Spec)
                .collect(),
            sources: extracted_specs.sources,
        })
    }
}

/// Converts the input variant configuration passed from pixi to something that
/// rattler build can deal with.
pub fn convert_input_variant_configuration(
    variants: Option<BTreeMap<String, Vec<String>>>,
) -> Option<BTreeMap<NormalizedKey, Vec<Variable>>> {
    variants.map(|v| {
        v.into_iter()
            .map(|(k, v)| {
                (
                    k.into(),
                    v.into_iter().map(|v| Variable::from_string(&v)).collect(),
                )
            })
            .collect()
    })
}

#[derive(Debug, Error, Diagnostic)]
pub enum ConvertDependencyError {
    #[error("only matchspecs with defined package names are supported")]
    MissingName,

    #[error("could not parse version spec for variant key {0}: {1}")]
    VariantSpecParseError(String, rattler_conda_types::ParseMatchSpecError),

    #[error("could not apply pin. The following subpackage is not available: {0:?}")]
    SubpackageNotFound(PackageName),

    #[error("could not apply pin: {0}")]
    PinApplyError(PinError),
}

fn convert_nameless_matchspec(spec: NamelessMatchSpec) -> pbt::BinaryPackageSpecV1 {
    pbt::BinaryPackageSpecV1 {
        version: spec.version,
        build: spec.build,
        build_number: spec.build_number,
        file_name: spec.file_name,
        channel: spec.channel.map(|c| c.base_url.clone().into()),
        subdir: spec.subdir,
        md5: spec.md5,
        sha256: spec.sha256,
        url: spec.url,
        license: spec.license,
    }
}

/// Checks if it is applicable to apply a variant on the specified match spec. A
/// variant can be applied if it has a name and no other fields set. Returns the
/// name of the variant that should be used.
fn can_apply_variant(spec: &MatchSpec) -> Option<&PackageName> {
    match &spec {
        MatchSpec {
            name: Some(name),
            version: None,
            build: None,
            build_number: None,
            file_name: None,
            extras: None,
            channel: None,
            subdir: None,
            namespace: None,
            md5: None,
            sha256: None,
            license: None,
            url: None,
        } => Some(name),
        _ => None,
    }
}

fn apply_variant_and_convert(
    spec: &MatchSpec,
    variant: &BTreeMap<NormalizedKey, Variable>,
) -> Result<Option<NamedSpecV1<BinaryPackageSpecV1>>, ConvertDependencyError> {
    let Some(name) = can_apply_variant(spec) else {
        return Ok(None);
    };
    let Some(version) = variant.get(&name.into()).map(Variable::to_string) else {
        return Ok(None);
    };

    // if the variant starts with an alphanumeric character,
    // we have to add a '=' to the version spec
    let mut spec = version.to_string();

    // check if all characters are alphanumeric or ., in that case add
    // a '=' to get "startswith" behavior
    if spec.chars().all(|c| c.is_alphanumeric() || c == '.') {
        spec = format!("={spec}");
    }

    let variant = name.as_normalized().to_string();
    let spec: NamelessMatchSpec = spec
        .parse()
        .map_err(|e| ConvertDependencyError::VariantSpecParseError(variant.clone(), e))?;

    Ok(Some(pbt::NamedSpecV1 {
        name: name.as_source().to_owned(),
        spec: convert_nameless_matchspec(spec),
    }))
}

fn convert_dependency(
    dependency: Dependency,
    variant: &BTreeMap<NormalizedKey, Variable>,
    subpackages: &HashMap<PackageName, PackageIdentifier>,
    sources: &HashMap<String, pbt::SourcePackageSpecV1>,
) -> Result<pbt::NamedSpecV1<pbt::PackageSpecV1>, ConvertDependencyError> {
    let match_spec = match dependency {
        Dependency::Spec(spec) => {
            // Convert back to source spec if it is a source spec.
            if let Some(source_package) =
                spec.url.clone().and_then(from_source_url_to_source_package)
            {
                let Some(name) = spec.name else {
                    return Err(ConvertDependencyError::MissingName);
                };
                return Ok(pbt::NamedSpecV1 {
                    name: name.as_source().into(),
                    spec: pbt::PackageSpecV1::Source(source_package),
                });
            }

            // Apply a variant if it is applicable.
            if let Some(NamedSpecV1 { name, spec }) = apply_variant_and_convert(&spec, variant)? {
                return Ok(pbt::NamedSpecV1 {
                    name,
                    spec: pbt::PackageSpecV1::Binary(Box::new(spec)),
                });
            }
            spec
        }
        Dependency::PinSubpackage(pin) => {
            let name = &pin.pin_value().name;
            let subpackage = subpackages
                .get(name)
                .ok_or(ConvertDependencyError::SubpackageNotFound(name.to_owned()))?;
            pin.pin_value()
                .apply(&subpackage.version, &subpackage.build_string)
                .map_err(ConvertDependencyError::PinApplyError)?
        }
        _ => todo!("Handle other dependency types"),
    };

    let (Some(name), spec) = match_spec.into_nameless() else {
        return Err(ConvertDependencyError::MissingName);
    };

    if let Some(source) = sources
        .get(name.as_source())
        .or_else(|| sources.get(name.as_normalized()))
    {
        Ok(pbt::NamedSpecV1 {
            name: name.as_source().to_owned(),
            spec: pbt::PackageSpecV1::Source(source.clone()),
        })
    } else {
        Ok(pbt::NamedSpecV1 {
            name: name.as_source().to_owned(),
            spec: pbt::PackageSpecV1::Binary(Box::new(convert_nameless_matchspec(spec))),
        })
    }
}

fn convert_binary_dependency(
    dependency: Dependency,
    variant: &BTreeMap<NormalizedKey, Variable>,
    subpackages: &HashMap<PackageName, PackageIdentifier>,
) -> Result<pbt::NamedSpecV1<pbt::BinaryPackageSpecV1>, ConvertDependencyError> {
    let match_spec = match dependency {
        Dependency::Spec(spec) => {
            // Apply a variant if it is applicable.
            if let Some(spec) = apply_variant_and_convert(&spec, variant)? {
                return Ok(spec);
            }
            spec
        }
        Dependency::PinSubpackage(pin) => {
            let name = &pin.pin_value().name;
            let subpackage = subpackages
                .get(name)
                .ok_or(ConvertDependencyError::SubpackageNotFound(name.to_owned()))?;
            pin.pin_value()
                .apply(&subpackage.version, &subpackage.build_string)
                .map_err(ConvertDependencyError::PinApplyError)?
        }
        _ => todo!("Handle other dependency types"),
    };

    // Apply a variant if it is applicable.
    if let Some(spec) = apply_variant_and_convert(&match_spec, variant)? {
        return Ok(spec);
    }

    let (Some(name), spec) = match_spec.into_nameless() else {
        return Err(ConvertDependencyError::MissingName);
    };

    Ok(pbt::NamedSpecV1 {
        name: name.as_source().to_owned(),
        spec: convert_nameless_matchspec(spec),
    })
}

pub fn convert_dependencies(
    dependencies: Vec<Dependency>,
    variant: &BTreeMap<NormalizedKey, Variable>,
    subpackages: &HashMap<PackageName, PackageIdentifier>,
    sources: &HashMap<String, pbt::SourcePackageSpecV1>,
) -> Result<Vec<pbt::NamedSpecV1<pbt::PackageSpecV1>>, ConvertDependencyError> {
    dependencies
        .into_iter()
        .map(|spec| convert_dependency(spec, variant, subpackages, sources))
        .collect()
}

pub fn convert_binary_dependencies(
    dependencies: Vec<Dependency>,
    variant: &BTreeMap<NormalizedKey, Variable>,
    subpackages: &HashMap<PackageName, PackageIdentifier>,
) -> Result<Vec<pbt::NamedSpecV1<pbt::BinaryPackageSpecV1>>, ConvertDependencyError> {
    dependencies
        .into_iter()
        .map(|spec| convert_binary_dependency(spec, variant, subpackages))
        .collect()
}

/// Apply a variant to a dependency list and resolve all pin_subpackage and
/// compiler dependencies
pub fn apply_variant(
    raw_specs: &[Dependency],
    variant: &BTreeMap<NormalizedKey, Variable>,
    subpackages: &HashMap<PackageName, PackageIdentifier>,
    compatibility_specs: &HashMap<PackageName, PackageRecord>,
    build_time: bool,
) -> Result<Vec<DependencyInfo>, ResolveError> {
    raw_specs
        .iter()
        .map(|s| {
            match s {
                Dependency::Spec(m) => {
                    let m = m.clone();
                    if build_time && m.version.is_none() && m.build.is_none() {
                        if let Some(name) = &m.name {
                            if let Some(version) = variant.get(&name.into()) {
                                // if the variant starts with an alphanumeric character,
                                // we have to add a '=' to the version spec
                                let mut spec = version.to_string();

                                // check if all characters are alphanumeric or ., in that case add
                                // a '=' to get "startswith" behavior
                                if spec.chars().all(|c| c.is_alphanumeric() || c == '.') {
                                    spec = format!("={spec}");
                                }

                                let variant = name.as_normalized().to_string();
                                let spec: NamelessMatchSpec = spec.parse().map_err(|e| {
                                    ResolveError::VariantSpecParseError(variant.clone(), e)
                                })?;

                                let spec = MatchSpec::from_nameless(spec, Some(name.clone()));

                                return Ok(VariantDependency { spec, variant }.into());
                            }
                        }
                    }
                    Ok(SourceDependency { spec: m }.into())
                }
                Dependency::PinSubpackage(pin) => {
                    let name = &pin.pin_value().name;
                    let subpackage = subpackages
                        .get(name)
                        .ok_or(ResolveError::SubpackageNotFound(name.to_owned()))?;
                    let pinned = pin
                        .pin_value()
                        .apply(&subpackage.version, &subpackage.build_string)?;
                    Ok(PinSubpackageDependency {
                        spec: pinned,
                        name: name.as_normalized().to_string(),
                        args: pin.pin_value().args.clone(),
                    }
                    .into())
                }
                Dependency::PinCompatible(pin) => {
                    let name = &pin.pin_value().name;
                    let pin_package = compatibility_specs
                        .get(name)
                        .ok_or(ResolveError::SubpackageNotFound(name.to_owned()))?;

                    let pinned = pin
                        .pin_value()
                        .apply(&pin_package.version, &pin_package.build)?;
                    Ok(PinCompatibleDependency {
                        spec: pinned,
                        name: name.as_normalized().to_string(),
                        args: pin.pin_value().args.clone(),
                    }
                    .into())
                }
            }
        })
        .collect()
}
