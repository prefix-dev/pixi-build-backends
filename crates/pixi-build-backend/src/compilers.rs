//! We could expose the `default_compiler` function from the `rattler-build` crate

use rattler_conda_types::Platform;
use recipe_stage0::{
    matchspec::PackageDependency,
    recipe::{Conditional, Item, ListOrItem, Value},
};

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

pub fn compiler_requirements(language: &str) -> Vec<Item<PackageDependency>> {
    match language {
        "fortran" => vec!["gfortran".parse().unwrap()],
        lang if !["c", "cxx"].contains(&lang) => vec![language.parse().unwrap()],
        _ => {
            let mut items: Vec<Item<PackageDependency>> = vec![];

            // for windows
            let windows_compiler = match language {
                "c" => "vs2019",
                "cxx" => "vs2019",
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
                "c" => "clang",
                "cxx" => "clangxx",
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
                "c" => "emscripten",
                "cxx" => "emscripten",
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
                "c" => "gcc",
                "cxx" => "gxx",
                _ => unreachable!(),
            };

            let compiler_item: Value<PackageDependency> = default_compiler.parse().unwrap();
            items.push(Item::Value(compiler_item));

            items
        }
    }
}
