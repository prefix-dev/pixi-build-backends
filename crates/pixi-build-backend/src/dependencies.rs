use std::{
    collections::{BTreeMap, HashMap},
    str::FromStr,
};

use miette::{Context, IntoDiagnostic};
use pixi_build_types as pbt;
use rattler_build::{
    NormalizedKey,
    recipe::{parser::Dependency, variable::Variable},
};
use rattler_conda_types::{MatchSpec, NamelessMatchSpec, PackageName, ParseStrictness::Strict};

use crate::traits::PackageSpec;

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
