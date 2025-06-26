use indexmap::IndexMap;
use rattler_conda_types::PackageName;

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
/// What kind of dependency spec do we have
pub enum SpecType {
    /// Host dependencies are used that are needed by the host environment when
    /// running the project
    Host,
    /// Build dependencies are used when we need to build the project, may not
    /// be required at runtime
    Build,
    /// Regular dependencies that are used when we need to run the project
    Run,
}

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
