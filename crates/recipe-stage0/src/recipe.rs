use indexmap::IndexMap;
use pixi_build_types::{PackageSpecV1, ProjectModelV1, TargetsV1};
use rattler_build::recipe::parser::{About as RattlerBuildAbout, Script, ScriptContent};
use rattler_conda_types::{MatchSpec, PackageName, Platform, Version, VersionWithSource};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

use pixi_build_types::TargetV1;

use crate::matchspec::SerializableMatchSpec;

// Core enum for values that can be either concrete or templated
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value<T: ToString> {
    Concrete(T),
    Template(String), // Jinja template like "${{ name|lower }}"
}

impl<T: ToString> Value<T> {
    /// A dummy implementation of `ToString` for `Value<T>`.
    pub fn to_string(&self) -> String {
        match self {
            Value::Concrete(val) => val.to_string(),
            Value::Template(template) => template.clone(),
        }
    }

    pub fn concrete(&self) -> Option<&T> {
        if let Value::Concrete(val) = self {
            Some(val)
        } else {
            None
        }
    }
}

impl<T: ToString> ToString for Value<T> {
    fn to_string(&self) -> String {
        match self {
            Value::Concrete(val) => val.to_string().clone(),
            Value::Template(template) => template.clone(),
        }
    }
}

// Any item in a list can be either a value or a conditional
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Item<T: ToString> {
    Value(Value<T>),
    Conditional(Conditional<T>),
}

#[derive(Debug, Clone)]
pub struct ListOrItem<T: ToString>(pub Vec<T>);

impl<T: ToString> serde::Serialize for ListOrItem<T>
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

impl<'de, T: ToString> serde::Deserialize<'de> for ListOrItem<T>
where
    T: serde::Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, Visitor};
        use std::fmt;

        struct ListOrItemVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T: ToString + serde::Deserialize<'de>> Visitor<'de> for ListOrItemVisitor<T> {
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

impl<T: ToString> ToString for ListOrItem<T> {
    fn to_string(&self) -> String {
        match self.0.len() {
            0 => "[]".to_string(),
            1 => self.0[0].to_string(),
            _ => format!(
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

impl<T: ToString> ListOrItem<T> {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conditional<T: ToString> {
    #[serde(rename = "if")]
    pub condition: String,
    pub then: ListOrItem<T>,
    #[serde(rename = "else")]
    pub else_value: Option<T>,
}

// Type alias for lists that can contain conditionals
pub type ConditionalList<T> = Vec<Item<T>>;

// Main recipe structure
#[derive(Debug, Serialize, Deserialize)]
pub struct IntermediateRecipe {
    pub context: Option<HashMap<String, Value<String>>>,
    pub package: Package,
    pub source: Option<Source>,
    pub build: Option<Build>,
    pub requirements: Option<ConditionalRequirements>,
    pub tests: Option<Vec<Test>>,
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
    /// FIXME: right now it is getting only the build dependencies
    /// and it does not evaluate the variants.

    /// Converts the recipe to the `rendered` rattler-build format.
    /// This means that we need to evaluate the variants and conditionals
    #[allow(dead_code)]
    pub fn into_recipe(self, _platform: Platform) -> rattler_build::recipe::Recipe {
        let context = {
            // let context = self.context.unwrap_or_default();
            IndexMap::default()
        };

        let package = {
            let name = self.package.name.clone();
            let version = self.package.version.clone();

            // Convert the package name and version into the rattler-build format
            rattler_build::recipe::parser::Package {
                name: PackageName::try_from(name.to_string()).unwrap(),
                version: VersionWithSource::from_str(&version.to_string()).unwrap(),
            }
        };

        let source = self.source.as_ref().map(|_source| {
            // Convert the source into the rattler-build format

            rattler_build::recipe::parser::GitSource {
                url: rattler_build::recipe::parser::GitUrl::Url(
                    // Url::from_str(source.url.to_string().as_str()).unwrap(),
                    Url::from_str("some_url").unwrap(),
                ),
                rev: rattler_build::recipe::parser::GitRev::Head,
                depth: None,
                patches: vec![],
                target_directory: None,
                lfs: false,
            }
        });

        let build = self.build.as_ref().map(|build| {
            let script = "$PYTHON -m pip install --ignore-installed {{ COMMON_OPTIONS }} $SRC_DIR";
            let script_content = ScriptContent::Command(script.to_string());

            // Convert the build into the rattler-build format
            rattler_build::recipe::parser::Build {
                number: build
                    .number
                    .as_ref()
                    .map(|n| n.concrete().cloned().unwrap())
                    .unwrap_or(1),
                script: Script::from(script_content),
                ..Default::default()
            }
        });

        let requirements = {
            let dependencies = self.dependencies(Platform::OsxArm64).unwrap();

            rattler_build::recipe::parser::Requirements {
                build: dependencies
                    .build
                    .map(|list| {
                        list.into_iter()
                            .map(|spec| rattler_build::recipe::parser::Dependency::Spec(spec.0))
                            .collect()
                    })
                    .unwrap_or_default(),
                ..Default::default()
            }
        };

        // Convert the recipe into the rattler-build format
        rattler_build::recipe::Recipe {
            schema_version: 1,
            context,
            package,
            cache: None,
            source: vec![rattler_build::recipe::parser::Source::Git(source.unwrap())],
            build: build.unwrap(),

            requirements,
            tests: vec![],
            about: RattlerBuildAbout::default(),
            extra: Default::default(),
        }
    }

    /// FIXME: right now it is getting only the build dependencies
    /// and it does not evaluate the variants.
    ///
    ///
    /// Evaluates the requirements for a specific platform.
    #[allow(dead_code)]
    pub fn dependencies(&self, _platform: Platform) -> Option<EvaluatedDependencies> {
        let requirements = self.requirements.as_ref()?;

        let build = requirements.build.as_ref().map(|list| {
            list.iter()
                .filter_map(|item| match item {
                    Item::Value(Value::Concrete(spec)) => Some(spec.clone()),
                    Item::Value(Value::Template(template)) => {
                        unimplemented!("Template evaluation not implemented yet: {}", template);
                    }
                    Item::Conditional(cond) => {
                        // Evaluate the condition and return the then value if true
                        // FIXME: for now we just simplify the condition check
                        if cond.condition == "unix" {
                            let spec = SerializableMatchSpec::from_str(&cond.then.to_string())
                                .expect("Invalid MatchSpec in conditional");
                            Some(spec)
                        } else {
                            let spec = SerializableMatchSpec::from_str(
                                &cond.else_value.as_ref()?.to_string(),
                            )
                            .expect("Invalid MatchSpec in conditional else");
                            Some(spec)
                            // cond.else_value.clone()
                        }
                    }
                })
                .collect()
        });

        let evaluated_dependencies = EvaluatedDependencies {
            build,
            host: None,            // TODO: Implement host dependencies
            run: None,             // TODO: Implement run dependencies
            run_constraints: None, // TODO: Implement run constraints
        };
        Some(evaluated_dependencies)
    }

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

        let source = Source::path(manifest_root.display().to_string(), None);

        let conditional_requirements = into_target_requirements(&model.targets.unwrap_or_default());

        let requirements = conditional_requirements.into_conditional_requirements();

        IntermediateRecipe {
            context: None,
            package,
            source: Some(source),
            build: None,
            requirements: Some(requirements),
            tests: None,
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

pub fn into_target_requirements(targets: &TargetsV1) -> TargetRequirements {
    let mut target_map = HashMap::new();

    // Add default target
    if let Some(default_target) = &targets.default_target {
        target_map.insert(Target::Default, target_spec_to_requirements(default_target));
    }

    // Add specific targets
    if let Some(specific_targets) = &targets.targets {
        for (selector, target) in specific_targets {
            let requirements = target_spec_to_requirements(target);
            target_map.insert(Target::Specific(selector.to_string()), requirements);
        }
    }

    TargetRequirements { target: target_map }
}

pub(crate) fn target_spec_to_requirements(target: &TargetV1) -> Requirements {
    Requirements {
        build: target.clone().build_dependencies.map(|deps| {
            package_specs_to_match_spec(deps)
                .into_iter()
                .map(SerializableMatchSpec::from)
                .map(Value::Concrete)
                .collect()
        }),
        host: target.clone().host_dependencies.map(|deps| {
            package_specs_to_match_spec(deps)
                .into_iter()
                .map(SerializableMatchSpec::from)
                .map(Value::Concrete)
                .collect()
        }),
        run: target.clone().run_dependencies.map(|deps| {
            package_specs_to_match_spec(deps)
                .into_iter()
                .map(SerializableMatchSpec::from)
                .map(Value::Concrete)
                .collect()
        }),
        run_constraints: None,
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Package {
    pub name: Value<String>,
    pub version: Value<String>,
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
    pub fn url(url: String, sha256: Option<String>) -> Self {
        Source::Url(UrlSource {
            url: Value::Concrete(url),
            sha256: sha256.map(Value::Concrete),
        })
    }

    pub fn path(path: String, sha256: Option<String>) -> Self {
        Source::Path(PathSource {
            path: Value::Concrete(path),
            sha256: sha256.map(Value::Concrete),
        })
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Build {
    pub number: Option<Value<u64>>,
    pub script: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub enum Target {
    Default,
    Specific(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TargetRequirements {
    pub target: HashMap<Target, Requirements>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConditionalRequirements {
    pub build: Option<ConditionalList<SerializableMatchSpec>>,
    pub host: Option<ConditionalList<SerializableMatchSpec>>,
    pub run: Option<ConditionalList<SerializableMatchSpec>>,
    pub run_constraints: Option<ConditionalList<SerializableMatchSpec>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Requirements {
    pub build: Option<Vec<Value<SerializableMatchSpec>>>,
    pub host: Option<Vec<Value<SerializableMatchSpec>>>,
    pub run: Option<Vec<Value<SerializableMatchSpec>>>,
    pub run_constraints: Option<Vec<Value<SerializableMatchSpec>>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Test {
    pub package_contents: Option<PackageContents>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PackageContents {
    pub include: Option<ConditionalList<String>>,
    pub files: Option<ConditionalList<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct About {
    pub homepage: Option<Value<String>>,
    pub license: Option<Value<String>>,
    pub license_file: Option<Value<String>>,
    pub summary: Option<Value<String>>,
    pub description: Option<Value<String>>,
    pub documentation: Option<Value<String>>,
    pub repository: Option<Value<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Extra {
    #[serde(rename = "recipe-maintainers")]
    pub recipe_maintainers: Option<ConditionalList<String>>,
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

// Helper implementations
impl<T: ToString> Item<T> {
    pub fn value(val: T) -> Self {
        Item::Value(Value::Concrete(val))
    }

    pub fn template(template: String) -> Self {
        Item::Value(Value::Template(template))
    }

    pub fn conditional(condition: String, then_value: ListOrItem<T>) -> Self {
        Item::Conditional(Conditional::new(condition, then_value))
    }
}

impl<T: ToString> Conditional<T> {
    pub fn new(condition: String, then_value: ListOrItem<T>) -> Self {
        Self {
            condition,
            then: then_value,
            else_value: None,
        }
    }

    pub fn with_else(mut self, else_value: T) -> Self {
        self.else_value = Some(else_value);
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

pub fn main() {
    // Create your recipe
    let recipe = IntermediateRecipe {
        context: None,
        package: Package {
            name: Value::Concrete("example-package".to_string()),
            version: Value::Concrete("1.0.0".to_string()),
        },
        source: Some(Source::url(
            "https://example.com/source.tar.gz".to_string(),
            None,
        )),
        build: Some(Build {
            number: None,
            script: vec!["echo Building...".to_string()],
        }),
        requirements: None,
        tests: None,
        about: None,
        extra: None,
    };

    // Convert to YAML
    match recipe.to_yaml() {
        Ok(yaml_string) => {
            println!("{}", yaml_string);
            // Write to file, send over network, etc.
        }
        Err(e) => eprintln!("Failed to serialize: {}", e),
    }
}

pub(crate) fn into_conditional_list(
    target: Target,
    values: Vec<Value<SerializableMatchSpec>>,
) -> ConditionalList<SerializableMatchSpec> {
    let mut result: Vec<Item<_>> = ConditionalList::new();

    match target {
        // Default target, add it directly
        Target::Default => {
            // result.push(Item::Value(item))
            values.iter().for_each(|item| {
                result.push(Item::Value(item.clone()));
            });
        }

        Target::Specific(ref cond) => {
            // Wrap in conditional if not already conditional
            let conditional_item = Item::Conditional(Conditional {
                condition: cond.clone(),
                then: ListOrItem::new(
                    values
                        .iter()
                        .map(|v| v.concrete().unwrap().clone())
                        .collect(),
                ),
                else_value: None,
            });

            result.push(conditional_item);
        }
    }
    result
}

/// Transform TargetRequirements into a single Requirements struct
/// by converting target-specific requirements into conditional lists
impl TargetRequirements {
    pub fn into_conditional_requirements(self) -> ConditionalRequirements {
        let mut build_items = Vec::new();
        let mut host_items = Vec::new();
        let mut run_items = Vec::new();
        let mut run_constraints_items = Vec::new();

        // Process each target and its requirements
        for (target, requirements) in self.target {
            // Process build dependencies
            if let Some(build_deps) = requirements.build {
                build_items.extend(into_conditional_list(target.clone(), build_deps));
            }

            // Process host dependencies
            if let Some(host_deps) = requirements.host {
                host_items.extend(into_conditional_list(target.clone(), host_deps));
            }

            // Process run dependencies
            if let Some(run_deps) = requirements.run {
                run_items.extend(into_conditional_list(target.clone(), run_deps));
            }

            // Process run constraints
            if let Some(run_constraints_deps) = requirements.run_constraints {
                run_constraints_items
                    .extend(into_conditional_list(target.clone(), run_constraints_deps));
            }
        }

        ConditionalRequirements {
            build: if build_items.is_empty() {
                None
            } else {
                Some(build_items)
            },
            host: if host_items.is_empty() {
                None
            } else {
                Some(host_items)
            },
            run: if run_items.is_empty() {
                None
            } else {
                Some(run_items)
            },
            run_constraints: if run_constraints_items.is_empty() {
                None
            } else {
                Some(run_constraints_items)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    // use rattler_build::assert_miette_snapshot;

    use pixi_build_types::{TargetSelectorV1, TargetsV1};

    use crate::marked_yaml::ToMarkedYaml;

    use super::*;
    // TODO: write a unit test that can convert a project model into a IR and then to recipe.yaml

    #[test]
    fn test_recipe_to_yaml() {
        // Create a simple recipe
        let mut context = HashMap::new();
        context.insert("name".to_string(), Value::Concrete("xtensor".to_string()));
        context.insert("version".to_string(), Value::Concrete("0.24.6".to_string()));

        let recipe = IntermediateRecipe {
            context: Some(context),
            package: Package {
                name: Value::Template("${{ name|lower }}".to_string()),
                version: Value::Template("${{ version }}".to_string()),
            },
            source: Some(Source::url(
                "https://github.com/xtensor-stack/xtensor/archive/${{ version }}.tar.gz"
                    .to_string(),
                Some(
                    "f87259b51aabafdd1183947747edfff4cff75d55375334f2e81cee6dc68ef655".to_string(),
                ),
            )),
            build: Some(Build {
                number: Some(Value::Concrete(0)),
                script: vec![],
            }),
            requirements: Some(ConditionalRequirements {
                build: Some(vec![
                    Item::template("${{ compiler('cxx') }}".to_string()),
                    Item::value("cmake".parse().unwrap()),
                    Item::Conditional(
                        Conditional::new(
                            "unix".to_string(),
                            ListOrItem::single("make".parse().unwrap()),
                        )
                        .with_else("ninja".parse().unwrap()),
                    ),
                ]),
                host: Some(vec![Item::value("xtl >=0.7,<0.8".parse().unwrap())]),
                run: Some(vec![Item::value("xtl >=0.7,<0.8".parse().unwrap())]),
                run_constraints: Some(vec![Item::value("xsimd >=8.0.3,<10".parse().unwrap())]),
            }),
            tests: None,
            about: Some(About {
                homepage: Some(Value::Concrete(
                    "https://github.com/xtensor-stack/xtensor".to_string(),
                )),
                license: Some(Value::Concrete("BSD-3-Clause".to_string())),
                license_file: Some(Value::Concrete("LICENSE".to_string())),
                summary: Some(Value::Concrete(
                    "The C++ tensor algebra library".to_string(),
                )),
                description: Some(Value::Concrete(
                    "Multi dimensional arrays with broadcasting and lazy computing".to_string(),
                )),
                documentation: Some(Value::Concrete(
                    "https://xtensor.readthedocs.io".to_string(),
                )),
                repository: Some(Value::Concrete(
                    "https://github.com/xtensor-stack/xtensor".to_string(),
                )),
            }),
            extra: Some(Extra {
                recipe_maintainers: Some(vec![Item::value("some-maintainer".to_string())]),
            }),
        };

        // Convert to YAML
        let yaml_result = recipe.to_yaml();
        assert!(yaml_result.is_ok());

        let yaml_string = yaml_result.unwrap();
        println!("Generated YAML:\n{}", yaml_string);

        // Check that we can convert it to marked_yaml
        let marked_yaml_result = recipe.to_marked_yaml();
        println!("Marked YAML:\n{:?}", marked_yaml_result);

        // Test round-trip: YAML -> Recipe -> YAML
        let parsed_recipe = IntermediateRecipe::from_yaml(&yaml_string);
        assert!(parsed_recipe.is_ok());
    }

    #[test]
    fn test_project_model_into_recipe() {
        // Create a dummy project model
        let model = ProjectModelV1 {
            name: "example-project".to_string(),
            version: Some(Version::from_str("1.0.0").unwrap()),
            targets: Some(TargetsV1 {
                default_target: Some(TargetV1 {
                    build_dependencies: Some(IndexMap::from([(
                        "boltons".to_string(),
                        PackageSpecV1::Binary(Box::new(
                            rattler_conda_types::VersionSpec::Any.into(),
                        )),
                    )])),
                    host_dependencies: Some(IndexMap::from([(
                        "boltons".to_string(),
                        PackageSpecV1::Binary(Box::new(
                            rattler_conda_types::VersionSpec::Any.into(),
                        )),
                    )])),
                    run_dependencies: Some(IndexMap::from([(
                        "boltons".to_string(),
                        PackageSpecV1::Binary(Box::new(
                            rattler_conda_types::VersionSpec::Any.into(),
                        )),
                    )])),
                }),
                targets: Some(HashMap::from([(
                    TargetSelectorV1::Unix,
                    TargetV1 {
                        host_dependencies: Some(IndexMap::from([(
                            "rich".to_string(),
                            PackageSpecV1::Binary(Box::new(
                                rattler_conda_types::VersionSpec::Any.into(),
                            )),
                        )])),
                        build_dependencies: Some(IndexMap::from([
                            (
                                "rich".to_string(),
                                PackageSpecV1::Binary(Box::new(
                                    rattler_conda_types::VersionSpec::Any.into(),
                                )),
                            ),
                            (
                                "cowpy".to_string(),
                                PackageSpecV1::Binary(Box::new(
                                    rattler_conda_types::VersionSpec::Any.into(),
                                )),
                            ),
                        ])),
                        run_dependencies: None,
                    },
                )])),
            }),
            description: None,
            license: None,
            license_file: None,
            homepage: None,
            repository: None,
            documentation: None,
            authors: None,
            readme: None,
        };

        // Convert to IR
        let ir = IntermediateRecipe::from_model(model, PathBuf::from("/path/to/manifest"));

        insta::assert_yaml_snapshot!(ir)
    }

    #[test]
    fn test_target_requirements_transformation() {
        // Create a TargetRequirements with default and specific targets
        let mut target_map = HashMap::new();

        // Default target with build dependencies
        target_map.insert(
            Target::Default,
            Requirements {
                build: Some(vec![
                    Value::Concrete("cmake".parse().unwrap()),
                    Value::Concrete("make".parse().unwrap()),
                ]),
                host: Some(vec![Value::Concrete("python".parse().unwrap())]),
                run: None,
                run_constraints: None,
            },
        );

        // Unix-specific target
        target_map.insert(
            Target::Specific("unix".to_string()),
            Requirements {
                build: Some(vec![Value::Concrete("gcc".parse().unwrap())]),
                host: Some(vec![Value::Concrete("libssl".parse().unwrap())]),
                run: Some(vec![Value::Concrete("openssl".parse().unwrap())]),
                run_constraints: None,
            },
        );

        // Windows-specific target
        target_map.insert(
            Target::Specific("win".to_string()),
            Requirements {
                build: Some(vec![Value::Concrete("msvc".parse().unwrap())]),
                host: None,
                run: Some(vec![Value::Concrete("vcredist".parse().unwrap())]),
                run_constraints: None,
            },
        );

        let target_requirements = TargetRequirements { target: target_map };

        // Transform to Requirements
        let requirements = target_requirements.into_conditional_requirements();

        // Check that build dependencies are correctly combined
        assert!(requirements.build.is_some());
        let build_deps = &requirements.build.as_ref().unwrap();
        assert_eq!(build_deps.len(), 4); // cmake, make (default) + gcc (unix) + msvc (win)

        // Check that host dependencies are correctly combined
        assert!(requirements.host.is_some());
        let host_deps = &requirements.host.as_ref().unwrap();
        assert_eq!(host_deps.len(), 2); // python (default) + libssl (unix)

        // Check that run dependencies are correctly combined
        assert!(requirements.run.is_some());
        let run_deps = &requirements.run.as_ref().unwrap();
        assert_eq!(run_deps.len(), 2); // openssl (unix) + vcredist (win)

        // Check that specific target dependencies are wrapped in conditionals
        let has_default = build_deps.iter().any(|item| matches!(item, Item::Value(_)));
        assert!(
            has_default,
            "Should have default dependencies without conditionals"
        );

        // Check that specific target dependencies are wrapped in conditionals
        let has_conditional = build_deps
            .iter()
            .any(|item| matches!(item, Item::Conditional(_)));
        assert!(
            has_conditional,
            "Should have conditional dependencies for specific targets"
        );

        println!("Transformed requirements: {:#?}", requirements);
    }
}
