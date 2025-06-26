use indexmap::IndexMap;
use rattler_conda_types::PackageName;

/// A package spec dependency represent dependencies for a specific target.
#[derive(Debug, Clone)]
pub struct PackageSpecDependencies<T> {
    // /// Dependencies for this target.
    pub build: IndexMap<PackageName, T>,
    pub host: IndexMap<PackageName, T>,
    pub run: IndexMap<PackageName, T>,
    pub run_constraints: IndexMap<PackageName, T>,
}

impl<T> Default for PackageSpecDependencies<T> {
    fn default() -> Self {
        PackageSpecDependencies {
            build: IndexMap::new(),
            host: IndexMap::new(),
            run: IndexMap::new(),
            run_constraints: IndexMap::new(),
        }
    }
}
