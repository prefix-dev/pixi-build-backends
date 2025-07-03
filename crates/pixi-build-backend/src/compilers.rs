//! We could expose the `default_compiler` function from the `rattler-build` crate

use std::fmt::Display;

use rattler_conda_types::Platform;
use recipe_stage0::{matchspec::PackageDependency, recipe::Item};

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

pub fn default_compiler(platform: &Platform, language: &str) -> String {
    match language {
        // Platform agnostic compilers
        "fortran" => "gfortran",
        // Platform specific compilers
        "c" | "cxx" => {
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
        _ => language,
    }
    .to_string()
}

/// Returns the compiler template function for the specified language.
pub fn compiler_requirement(language: &Language) -> Item<PackageDependency> {
    format!("${{{{ compiler('{language}') }}}}")
        .parse()
        .expect("Failed to parse compiler requirement")
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_yaml_snapshot;

    #[test]
    fn test_compiler_requirements_fortran() {
        let result = compiler_requirement(&Language::Fortran);
        assert_yaml_snapshot!(result);
    }

    #[test]
    fn test_compiler_requirements_c() {
        let result = compiler_requirement(&Language::C);
        assert_yaml_snapshot!(result);
    }

    #[test]
    fn test_compiler_requirements_cxx() {
        let result = compiler_requirement(&Language::Cxx);
        assert_yaml_snapshot!(result);
    }

    #[test]
    fn test_compiler_requirements_rust() {
        let result = compiler_requirement(&Language::Other("rust"));
        assert_yaml_snapshot!(result);
    }

    #[test]
    fn test_compiler_requirements_python() {
        let result = compiler_requirement(&Language::Other("python"));
        assert_yaml_snapshot!(result);
    }
}
