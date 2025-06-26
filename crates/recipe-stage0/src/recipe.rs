use indexmap::IndexMap;
use pixi_build_types::{PackageSpecV1, ProjectModelV1, TargetsV1};
use rattler_conda_types::{MatchSpec, Version};
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};
use std::path::PathBuf;
use std::str::FromStr;

use pixi_build_types::TargetV1;

use crate::matchspec::SerializableMatchSpec;

// Core enum for values that can be either concrete or templated
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value<T> {
    Concrete(T),
    Template(String), // Jinja template like "${{ name|lower }}"
}

impl<T: ToString> Display for Value<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Concrete(val) => write!(f, "{}", val.to_string()),
            Value::Template(template) => write!(f, "{}", template),
        }
    }
}

impl<T: ToString> Value<T> {
    pub fn concrete(&self) -> Option<&T> {
        if let Value::Concrete(val) = self {
            Some(val)
        } else {
            None
        }
    }
}

impl<T: ToString + FromStr> FromStr for Value<T>
where
    T::Err: std::fmt::Display,
{
    type Err = T::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains("${{") {
            // If it contains some template syntax, treat it as a template
            return Ok(Value::Template(s.to_string()));
        }

        Ok(Value::Concrete(T::from_str(s)?))
    }
}

impl From<SerializableMatchSpec> for Value<SerializableMatchSpec> {
    fn from(spec: SerializableMatchSpec) -> Self {
        Value::Concrete(spec)
    }
}

// Any item in a list can be either a value or a conditional
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Item<T> {
    Value(Value<T>),
    Conditional(Conditional<T>),
}

impl<T> From<Conditional<T>> for Item<T> {
    fn from(value: Conditional<T>) -> Self {
        Self::Conditional(value)
    }
}

impl From<Source> for Item<Source> {
    fn from(source: Source) -> Self {
        Item::Value(Value::Concrete(source))
    }
}

impl From<SerializableMatchSpec> for Item<SerializableMatchSpec> {
    fn from(matchspec: SerializableMatchSpec) -> Self {
        Item::Value(Value::Concrete(matchspec))
    }
}

impl<T: ToString + FromStr> FromStr for Item<T>
where
    T::Err: std::fmt::Display,
{
    type Err = T::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains("${{") {
            // If it contains some template syntax, treat it as a template
            return Ok(Item::Value(Value::Template(s.to_string())));
        }

        let value = Value::Concrete(T::from_str(s)?);
        Ok(Item::Value(value))
    }
}
#[derive(Clone, Default)]
pub struct ListOrItem<T>(pub Vec<T>);

impl<T: FromStr> FromStr for ListOrItem<T> {
    type Err = T::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ListOrItem::single(s.parse()?))
    }
}

impl<T> serde::Serialize for ListOrItem<T>
where
    T: serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.0.len() {
            1 => self.0[0].serialize(serializer),
            _ => self.0.serialize(serializer),
        }
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for ListOrItem<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, Visitor};
        use std::fmt;

        struct ListOrItemVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T: serde::Deserialize<'de>> Visitor<'de> for ListOrItemVisitor<T> {
            type Value = ListOrItem<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a single item or a list of items")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut vec = Vec::new();
                while let Some(item) = seq.next_element()? {
                    vec.push(item);
                }
                Ok(ListOrItem(vec))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                let item = T::deserialize(serde::de::value::StrDeserializer::new(value))?;
                Ok(ListOrItem(vec![item]))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: Error,
            {
                let item = T::deserialize(serde::de::value::StringDeserializer::new(value))?;
                Ok(ListOrItem(vec![item]))
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let item = T::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(ListOrItem(vec![item]))
            }
        }

        deserializer.deserialize_any(ListOrItemVisitor(std::marker::PhantomData))
    }
}

impl<T: ToString> Display for ListOrItem<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.len() {
            0 => write!(f, "[]"),
            1 => write!(f, "{}", self.0[0].to_string()),
            _ => write!(
                f,
                "[{}]",
                self.0
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl<T> ListOrItem<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self(items)
    }

    pub fn single(item: T) -> Self {
        Self(vec![item])
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> std::slice::Iter<T> {
        self.0.iter()
    }
}

// Conditional structure for if-else logic
#[derive(Clone, Serialize, Deserialize)]
pub struct Conditional<T> {
    #[serde(rename = "if")]
    pub condition: String,
    pub then: ListOrItem<T>,
    #[serde(rename = "else")]
    pub else_value: ListOrItem<T>,
}

// Type alias for lists that can contain conditionals
pub type ConditionalList<T> = Vec<Item<T>>;

// #[derive(Debug, Serialize, Deserialize, Default)]
// #[serde(untagged)]
// pub struct Sources {
//     pub sources: ConditionalList<Source>,
// }

// Main recipe structure
#[derive(Serialize, Deserialize, Default)]
pub struct IntermediateRecipe {
    pub context: IndexMap<String, Value<String>>,
    pub package: Package,
    pub source: ConditionalList<Source>,
    pub build: Build,
    pub requirements: ConditionalRequirements,
    pub tests: Vec<Test>,
    pub about: Option<About>,
    pub extra: Option<Extra>,
}

pub struct EvaluatedDependencies {
    pub build: Option<Vec<SerializableMatchSpec>>,
    pub host: Option<Vec<SerializableMatchSpec>>,
    pub run: Option<Vec<SerializableMatchSpec>>,
    pub run_constraints: Option<Vec<SerializableMatchSpec>>,
}

impl IntermediateRecipe {
    /// Creates a new IntermediateRecipe from a ProjectModelV1.
    pub fn from_model(model: ProjectModelV1, manifest_root: PathBuf) -> Self {
        let package = Package {
            name: Value::Concrete(model.name),
            version: Value::Concrete(
                model
                    .version
                    .unwrap_or_else(|| Version::from_str("0.1.0").unwrap())
                    .to_string(),
            ),
        };

        let source = ConditionalList::from(vec![Item::Value(Value::Concrete(Source::path(
            manifest_root.display().to_string(),
        )))]);

        let requirements = into_conditional_requirements(&model.targets.unwrap_or_default());

        IntermediateRecipe {
            context: Default::default(),
            package,
            source,
            build: Build::default(),
            requirements,
            tests: Default::default(),
            about: None,
            extra: None,
        }
    }
}

pub(crate) fn package_specs_to_match_spec(
    specs: IndexMap<String, PackageSpecV1>,
) -> Vec<MatchSpec> {
    specs
        .into_iter()
        .map(|(name, spec)| match spec {
            PackageSpecV1::Binary(_binary_spec) => {
                MatchSpec::from_str(name.as_str(), rattler_conda_types::ParseStrictness::Strict)
                    .unwrap()
            }
            PackageSpecV1::Source(source_spec) => {
                unimplemented!("Source dependencies not implemented yet: {:?}", source_spec)
            }
        })
        .collect()
}

pub(crate) fn into_conditional_requirements(targets: &TargetsV1) -> ConditionalRequirements {
    let mut build_items: ConditionalList<SerializableMatchSpec> = ConditionalList::new();
    let mut host_items = ConditionalList::new();
    let mut run_items = ConditionalList::new();
    let mut run_constraints_items = ConditionalList::new();

    // Add default target
    if let Some(default_target) = &targets.default_target {
        let default_requirements = target_spec_to_requirements(default_target);
        build_items.extend(default_requirements.build.iter().cloned().map(Into::into));
        host_items.extend(default_requirements.host.iter().cloned().map(Into::into));
        run_items.extend(default_requirements.run.iter().cloned().map(Into::into));
        run_constraints_items.extend(
            default_requirements
                .run_constraints
                .iter()
                .cloned()
                .map(Into::into),
        );
    }

    // Add specific targets
    if let Some(specific_targets) = &targets.targets {
        for (selector, target) in specific_targets {
            let requirements = target_spec_to_requirements(target);
            build_items.extend(requirements.build.iter().cloned().map(|spec| {
                Conditional {
                    condition: selector.to_string(),
                    then: ListOrItem(vec![spec]),
                    else_value: ListOrItem::default(),
                }
                .into()
            }));
            host_items.extend(requirements.host.iter().cloned().map(|spec| {
                Conditional {
                    condition: selector.to_string(),
                    then: ListOrItem(vec![spec]),
                    else_value: ListOrItem::default(),
                }
                .into()
            }));

            run_items.extend(requirements.run.iter().cloned().map(|spec| {
                Conditional {
                    condition: selector.to_string(),
                    then: ListOrItem(vec![spec]),
                    else_value: ListOrItem::default(),
                }
                .into()
            }));
            run_constraints_items.extend(requirements.run_constraints.iter().cloned().map(
                |spec| {
                    Conditional {
                        condition: selector.to_string(),
                        then: ListOrItem(vec![spec]),
                        else_value: ListOrItem::default(),
                    }
                    .into()
                },
            ));
        }
    }

    ConditionalRequirements {
        build: build_items,
        host: host_items,
        run: run_items,
        run_constraints: run_constraints_items,
    }
}

// TODO: Should it be a From implementation?
pub(crate) fn target_spec_to_requirements(target: &TargetV1) -> Requirements {
    Requirements {
        build: target
            .clone()
            .build_dependencies
            .map(|deps| {
                package_specs_to_match_spec(deps)
                    .into_iter()
                    .map(SerializableMatchSpec::from)
                    .collect()
            })
            .unwrap_or_default(),
        host: target
            .clone()
            .host_dependencies
            .map(|deps| {
                package_specs_to_match_spec(deps)
                    .into_iter()
                    .map(SerializableMatchSpec::from)
                    .collect()
            })
            .unwrap_or_default(),
        run: target
            .clone()
            .run_dependencies
            .map(|deps| {
                package_specs_to_match_spec(deps)
                    .into_iter()
                    .map(SerializableMatchSpec::from)
                    .collect()
            })
            .unwrap_or_default(),
        run_constraints: vec![],
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Package {
    pub name: Value<String>,
    pub version: Value<String>,
}

impl Default for Package {
    fn default() -> Self {
        Package {
            name: Value::Concrete("default-package".to_string()),
            version: Value::Concrete("0.0.1".to_string()),
        }
    }
}

/// Source information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Source {
    /// Url source pointing to a tarball or similar to retrieve the source from
    Url(UrlSource),
    /// Path source pointing to a local path where the source can be found
    Path(PathSource),
}

impl Source {
    pub fn url(url: String) -> Self {
        Source::Url(UrlSource {
            url: Value::Concrete(url),
            sha256: None,
        })
    }

    pub fn path(path: String) -> Self {
        Source::Path(PathSource {
            path: Value::Concrete(path),
            sha256: None,
        })
    }

    pub fn with_sha256(self, sha256: String) -> Self {
        match self {
            Source::Url(mut url_source) => {
                url_source.sha256 = Some(Value::Concrete(sha256));
                Source::Url(url_source)
            }
            Source::Path(mut path_source) => {
                path_source.sha256 = Some(Value::Concrete(sha256));
                Source::Path(path_source)
            }
        }
    }
}

impl From<UrlSource> for Source {
    fn from(url_source: UrlSource) -> Self {
        Source::Url(url_source)
    }
}
impl From<PathSource> for Source {
    fn from(path_source: PathSource) -> Self {
        Source::Path(path_source)
    }
}

impl Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Url(url_source) => {
                let sha256 = url_source
                    .sha256
                    .as_ref()
                    .map_or("".to_string(), |s| s.to_string());
                write!(f, "url: {}, sha256: {}", url_source.url, sha256)
            }
            Source::Path(path_source) => {
                let sha256 = path_source
                    .sha256
                    .as_ref()
                    .map_or("".to_string(), |s| s.to_string());
                write!(f, "path: {}, sha256: {}", path_source.path, sha256)
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UrlSource {
    pub url: Value<String>,
    pub sha256: Option<Value<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PathSource {
    pub path: Value<String>,
    pub sha256: Option<Value<String>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Build {
    pub number: Option<Value<u64>>,
    pub script: Vec<String>,
}

impl Build {
    pub fn new(script: Vec<String>) -> Self {
        Build {
            number: None,
            script,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub enum Target {
    Default,
    Specific(String),
}

#[derive(Serialize, Deserialize, Default)]
pub struct ConditionalRequirements {
    pub build: ConditionalList<SerializableMatchSpec>,
    pub host: ConditionalList<SerializableMatchSpec>,
    pub run: ConditionalList<SerializableMatchSpec>,
    pub run_constraints: ConditionalList<SerializableMatchSpec>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Requirements {
    pub build: Vec<SerializableMatchSpec>,
    pub host: Vec<SerializableMatchSpec>,
    pub run: Vec<SerializableMatchSpec>,
    pub run_constraints: Vec<SerializableMatchSpec>,
}

#[derive(Serialize, Deserialize)]
pub struct Test {
    pub package_contents: Option<PackageContents>,
}

#[derive(Serialize, Deserialize)]
pub struct PackageContents {
    pub include: Option<ConditionalList<String>>,
    pub files: Option<ConditionalList<String>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct About {
    pub homepage: Option<Value<String>>,
    pub license: Option<Value<String>>,
    pub license_file: Option<Value<String>>,
    pub summary: Option<Value<String>>,
    pub description: Option<Value<String>>,
    pub documentation: Option<Value<String>>,
    pub repository: Option<Value<String>>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Extra {
    #[serde(rename = "recipe-maintainers")]
    pub recipe_maintainers: ConditionalList<String>,
}

// Implementation for Recipe
impl IntermediateRecipe {
    /// Converts the recipe to YAML string
    pub fn to_yaml(&self) -> Result<String, serde_yaml::Error> {
        serde_yaml::to_string(self)
    }

    /// Converts the recipe to pretty-formatted YAML string
    pub fn to_yaml_pretty(&self) -> Result<String, serde_yaml::Error> {
        // serde_yaml doesn't have a "pretty" option like serde_json,
        // but it produces readable YAML by default
        self.to_yaml()
    }

    /// Creates a recipe from YAML string
    pub fn from_yaml(yaml: &str) -> Result<IntermediateRecipe, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }
}

impl<T: ToString + Default + Debug> Conditional<T> {
    pub fn new(condition: String, then_value: ListOrItem<T>) -> Self {
        Self {
            condition,
            then: then_value,
            else_value: ListOrItem::default(),
        }
    }

    pub fn with_else(mut self, else_value: ListOrItem<T>) -> Self {
        self.else_value = else_value;
        self
    }
}

impl<T: ToString> Value<T> {
    pub fn is_template(&self) -> bool {
        matches!(self, Value::Template(_))
    }

    pub fn is_concrete(&self) -> bool {
        matches!(self, Value::Concrete(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recipe_to_yaml() {
        // Create a simple recipe
        let mut context = IndexMap::new();
        context.insert("name".to_string(), Value::Concrete("xtensor".to_string()));
        context.insert("version".to_string(), Value::Concrete("0.24.6".to_string()));

        let source = ConditionalList::from(vec![<Source as Into<Item<Source>>>::into(
            UrlSource {
                url: "https://github.com/xtensor-stack/xtensor/archive/${{ version }}.tar.gz"
                    .parse()
                    .unwrap(),
                sha256: Some(
                    "f87259b51aabafdd1183947747edfff4cff75d55375334f2e81cee6dc68ef655"
                        .parse()
                        .unwrap(),
                ),
            }
            .into(),
        )]);

        let recipe = IntermediateRecipe {
            context,
            package: Package {
                name: Value::Template("${{ name|lower }}".to_string()),
                version: Value::Template("${{ version }}".to_string()),
            },
            source,
            build: Build::default(),
            requirements: ConditionalRequirements {
                build: vec![
                    "${{ compiler('cxx') }}".parse().unwrap(),
                    "cmake".parse().unwrap(),
                    Conditional {
                        condition: "unix".to_owned(),
                        then: "make".parse().unwrap(),
                        else_value: "ninja".parse().unwrap(),
                    }
                    .into(),
                ],
                host: vec![
                    "xtl >=0.7,<0.8".parse().unwrap(),
                    "${{ context.name }}".parse().unwrap(),
                ],
                run: vec!["xtl >=0.7,<0.8".parse().unwrap()],
                run_constraints: vec!["xsimd >=8.0.3,<10".parse().unwrap()],
            },
            about: Some(About {
                homepage: Some(Value::Concrete(
                    "https://github.com/xtensor-stack/xtensor".to_string(),
                )),
                license: Some("BSD-3-Clause".parse().unwrap()),
                license_file: Some("LICENSE".parse().unwrap()),
                summary: Some("The C++ tensor algebra library".parse().unwrap()),
                description: Some(
                    "Multi dimensional arrays with broadcasting and lazy computing"
                        .parse()
                        .unwrap(),
                ),
                documentation: Some("https://xtensor.readthedocs.io".parse().unwrap()),
                repository: Some("https://github.com/xtensor-stack/xtensor".parse().unwrap()),
            }),
            extra: Some(Extra {
                recipe_maintainers: vec!["some-maintainer".parse().unwrap()],
            }),
            ..Default::default()
        };

        insta::assert_yaml_snapshot!(recipe)
    }
}
