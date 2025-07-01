//! We could expose the `default_compiler` function from the `rattler-build` crate

use std::fmt::Display;

use rattler_conda_types::Platform;
use recipe_stage0::{
    matchspec::PackageDependency,
    recipe::{Conditional, Item, ListOrItem, Value},
};

pub enum Language<'a> {
    C,
    Cxx,
    Fortran,
    Rust,
    Other(&'a str),
}

impl Display for Language<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::C => write!(f, "c"),
            Language::Cxx => write!(f, "cxx"),
            Language::Fortran => write!(f, "fortran"),
            Language::Rust => write!(f, "rust"),
            Language::Other(name) => write!(f, "{}", name),
        }
    }
}

pub fn default_compiler(platform: Platform, language: &str) -> Option<String> {
    Some(
        match language {
            // Platform agnostic compilers
            "fortran" => "gfortran",
            lang if !["c", "cxx"].contains(&lang) => lang,
            // Platform specific compilers
            _ => {
                if platform.is_windows() {
                    match language {
                        "c" => "vs2019",
                        "cxx" => "vs2019",
                        _ => unreachable!(),
                    }
                } else if platform.is_osx() {
                    match language {
                        "c" => "clang",
                        "cxx" => "clangxx",
                        _ => unreachable!(),
                    }
                } else if matches!(platform, Platform::EmscriptenWasm32) {
                    match language {
                        "c" => "emscripten",
                        "cxx" => "emscripten",
                        _ => unreachable!(),
                    }
                } else {
                    match language {
                        "c" => "gcc",
                        "cxx" => "gxx",
                        _ => unreachable!(),
                    }
                }
            }
        }
        .to_string(),
    )
}

/// Returns a list of compiler requirements that needs to be present in the final recipe
/// based on the specified language and platform.
/// For example, when building a fortran project, we will add `gfortran`
/// as a build requirement in the trecipe.
pub fn compiler_requirements(language: &Language) -> Vec<Item<PackageDependency>> {
    match language {
        Language::Fortran => vec!["gfortran".parse().unwrap()],
        Language::C | Language::Cxx => {
            let mut items: Vec<Item<PackageDependency>> = vec![];

            // for windows
            let windows_compiler = match language {
                Language::C => "vs2019",
                Language::Cxx => "vs2019",
                _ => unreachable!(),
            };

            let conditional = Conditional {
                condition: "win".to_string(),
                then: ListOrItem(vec![windows_compiler.parse().unwrap()]),
                else_value: ListOrItem::default(),
            };

            items.push(conditional.into());

            // for osx
            let osx_compiler = match language {
                Language::C => "clang",
                Language::Cxx => "clangxx",
                _ => unreachable!(),
            };

            let conditional = Conditional {
                condition: "osx".to_string(),
                then: ListOrItem(vec![osx_compiler.parse().unwrap()]),
                else_value: ListOrItem::default(),
            };

            items.push(conditional.into());

            // emscripten
            let emscripten_compiler = match language {
                Language::C => "emscripten",
                Language::Cxx => "emscripten",
                _ => unreachable!(),
            };

            let conditional = Conditional {
                condition: "emscripten".to_string(),
                then: ListOrItem(vec![emscripten_compiler.parse().unwrap()]),
                else_value: ListOrItem::default(),
            };

            items.push(conditional.into());

            // default compiler
            let default_compiler = match language {
                Language::C => "gcc",
                Language::Cxx => "gxx",
                _ => unreachable!(),
            };

            let compiler_item: Value<PackageDependency> = default_compiler.parse().unwrap();
            items.push(Item::Value(compiler_item));

            items
        }
        _ => vec![language.to_string().parse().unwrap()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_yaml_snapshot;

    #[test]
    fn test_compiler_requirements_fortran() {
        let result = compiler_requirements(&Language::Fortran);
        assert_yaml_snapshot!(result);
    }

    #[test]
    fn test_compiler_requirements_c() {
        let result = compiler_requirements(&Language::C);
        assert_yaml_snapshot!(result);
    }

    #[test]
    fn test_compiler_requirements_cxx() {
        let result = compiler_requirements(&Language::Cxx);
        assert_yaml_snapshot!(result);
    }

    #[test]
    fn test_compiler_requirements_rust() {
        let result = compiler_requirements(&Language::Other("rust"));
        assert_yaml_snapshot!(result);
    }

    #[test]
    fn test_compiler_requirements_python() {
        let result = compiler_requirements(&Language::Other("python"));
        assert_yaml_snapshot!(result);
    }
}
