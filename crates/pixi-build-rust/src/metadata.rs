use std::{collections::BTreeSet, path::PathBuf, str::FromStr, sync::Arc};

use cargo_toml::{
    AbstractFilesystem, Error as CargoTomlError, Filesystem, Inheritable, Manifest, Package,
    PackageTemplate,
};
use miette::Diagnostic;
use once_cell::unsync::OnceCell;
use pixi_build_backend::generated_recipe::MetadataProvider;
use rattler_conda_types::{ParseVersionError, Version};

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum MetadataError {
    #[error(transparent)]
    CargoTomlError(CargoTomlError),
    #[error("failed to parse version from Cargo.toml, {0}")]
    ParseVersionError(ParseVersionError),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error("missing inherited value from workspace {0}")]
    MissingInheritedValue(String),
}

/// An implementation of [`MetadataProvider`] that reads metadata from a
/// Cargo.toml file and possibly an associated workspace manifest.
pub struct CargoMetadataProvider {
    manifest_root: Arc<PathBuf>,
    cargo_manifest: OnceCell<Manifest>,
    workspace_manifest: OnceCell<(Manifest, PathBuf)>,
    ignore_cargo_manifest: bool,
}

impl CargoMetadataProvider {
    /// Constructs a new `CargoMetadataProvider` with the given manifest root.
    ///
    /// # Arguments
    ///
    /// * `manifest_root` - The directory that contains the `Cargo.toml` file
    /// * `ignore_cargo_manifest` - If `true`, all metadata methods will return `None`,
    ///   effectively disabling Cargo.toml metadata extraction
    pub fn new(manifest_root: impl Into<PathBuf>, ignore_cargo_manifest: bool) -> Self {
        Self {
            manifest_root: Arc::new(manifest_root.into()),
            cargo_manifest: OnceCell::default(),
            workspace_manifest: OnceCell::default(),
            ignore_cargo_manifest,
        }
    }

    /// Ensures that the manifest is loaded and returns the package metadata.
    fn ensure_manifest(&mut self) -> Result<Option<&Package>, MetadataError> {
        let manifest_root = self.manifest_root.clone();
        let manifest = self.cargo_manifest.get_or_try_init(move || {
            let cargo_toml_content = fs_err::read_to_string(manifest_root.join("Cargo.toml"))?;
            Manifest::from_slice_with_metadata(cargo_toml_content.as_bytes())
                .map_err(MetadataError::CargoTomlError)
        })?;
        Ok(manifest.package.as_ref())
    }

    /// Ensures that the workspace manifest is loaded, and returns the package
    /// template
    fn ensure_workspace_manifest(&mut self) -> Result<Option<&PackageTemplate>, MetadataError> {
        let manifest_root = self.manifest_root.clone();
        let manifest = self.ensure_manifest()?;
        let workspace_hint = manifest.and_then(|p| p.workspace.clone());
        let (manifest, _) = self.workspace_manifest.get_or_try_init(move || {
            Filesystem::new(&manifest_root)
                .parse_root_workspace(workspace_hint.as_deref())
                .map_err(MetadataError::CargoTomlError)
        })?;
        Ok(manifest.workspace.as_ref().and_then(|w| w.package.as_ref()))
    }

    /// Returns the set of globs that match files that influence the metadata of this package.
    ///
    /// This includes the package's own `Cargo.toml` file and any workspace `Cargo.toml` files
    /// if workspace inheritance is detected. These globs can be used for incremental builds
    /// to determine when metadata might have changed.
    ///
    /// # Returns
    ///
    /// A `BTreeSet` of glob patterns as strings. Common patterns include:
    /// - `"Cargo.toml"` - The package's manifest file
    /// - `"../../**/Cargo.toml"` - Workspace manifest files (when workspace inheritance is used)
    pub fn input_globs(&self) -> BTreeSet<String> {
        let mut input_globs = BTreeSet::new();

        let Some(_) = self.cargo_manifest.get() else {
            return input_globs;
        };

        // Add the Cargo.toml manifest file itself.
        input_globs.insert(String::from("Cargo.toml"));

        // If the manifest has workspace inheritance, we need to include a glob that
        // matches all Cargo.toml files up to the workspace root.
        if let Some((_, workspace_path)) = self.workspace_manifest.get() {
            if let Some(path) = pathdiff::diff_paths(
                workspace_path
                    .parent()
                    .expect("the workspace path is a file so it must have a parent"),
                self.manifest_root.as_ref(),
            ) {
                input_globs.insert(format!(
                    "{}/**/Cargo.toml",
                    path.display().to_string().replace("\\", "/")
                ));
            }
        }

        input_globs
    }
}

impl MetadataProvider for CargoMetadataProvider {
    type Error = MetadataError;

    /// Returns the package name from the Cargo.toml manifest.
    ///
    /// If `ignore_cargo_manifest` is true, returns `None`. Otherwise, extracts
    /// the name from the package section of the Cargo.toml file.
    fn name(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_cargo_manifest {
            return Ok(None);
        }
        Ok(self.ensure_manifest()?.map(|pkg| pkg.name.clone()))
    }

    /// Returns the package version from the Cargo.toml manifest.
    ///
    /// If `ignore_cargo_manifest` is true, returns `None`. Otherwise, extracts
    /// the version from the package section, handling workspace inheritance if needed.
    /// The version string is parsed into a `rattler_conda_types::Version`.
    fn version(&mut self) -> Result<Option<Version>, Self::Error> {
        if self.ignore_cargo_manifest {
            return Ok(None);
        }
        let Some(value) = self.ensure_manifest()?.map(|pkg| &pkg.version) else {
            return Ok(None);
        };
        let version = match value {
            Inheritable::Set(value) => value,
            Inheritable::Inherited => self
                .ensure_workspace_manifest()?
                .and_then(|template| template.version.as_ref())
                .ok_or_else(|| {
                    MetadataError::MissingInheritedValue(String::from("workspace.package.version"))
                })?,
        };
        Ok(Some(
            Version::from_str(version).map_err(MetadataError::ParseVersionError)?,
        ))
    }

    /// Returns the package description from the Cargo.toml manifest.
    ///
    /// If `ignore_cargo_manifest` is true, returns `None`. Otherwise, extracts
    /// the description from the package section, handling workspace inheritance if needed.
    fn description(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_cargo_manifest {
            return Ok(None);
        }
        let Some(value) = self.ensure_manifest()?.map(|pkg| &pkg.description) else {
            return Ok(None);
        };
        let description = match value {
            None => return Ok(None),
            Some(Inheritable::Set(value)) => value,
            Some(Inheritable::Inherited) => self
                .ensure_workspace_manifest()?
                .and_then(|template| template.description.as_ref())
                .ok_or_else(|| {
                    MetadataError::MissingInheritedValue(String::from(concat!(
                        "workspace.package.description",
                    )))
                })?,
        };
        Ok(Some(description.clone()))
    }

    /// Returns the package homepage URL from the Cargo.toml manifest.
    ///
    /// If `ignore_cargo_manifest` is true, returns `None`. Otherwise, extracts
    /// the homepage from the package section, handling workspace inheritance if needed.
    fn homepage(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_cargo_manifest {
            return Ok(None);
        }
        let Some(value) = self.ensure_manifest()?.map(|pkg| &pkg.homepage) else {
            return Ok(None);
        };
        let homepage = match value {
            None => return Ok(None),
            Some(Inheritable::Set(value)) => value,
            Some(Inheritable::Inherited) => self
                .ensure_workspace_manifest()?
                .and_then(|template| template.homepage.as_ref())
                .ok_or_else(|| {
                    MetadataError::MissingInheritedValue(String::from(concat!(
                        "workspace.package.homepage",
                    )))
                })?,
        };
        Ok(Some(homepage.clone()))
    }

    /// Returns the package license from the Cargo.toml manifest.
    ///
    /// If `ignore_cargo_manifest` is true, returns `None`. Otherwise, extracts
    /// the license from the package section, handling workspace inheritance if needed.
    fn license(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_cargo_manifest {
            return Ok(None);
        }
        let Some(value) = self.ensure_manifest()?.map(|pkg| &pkg.license) else {
            return Ok(None);
        };
        let license = match value {
            None => return Ok(None),
            Some(Inheritable::Set(value)) => value,
            Some(Inheritable::Inherited) => self
                .ensure_workspace_manifest()?
                .and_then(|template| template.license.as_ref())
                .ok_or_else(|| {
                    MetadataError::MissingInheritedValue(String::from(concat!(
                        "workspace.package.license",
                    )))
                })?,
        };
        Ok(Some(license.clone()))
    }

    /// Returns the package license file path from the Cargo.toml manifest.
    ///
    /// If `ignore_cargo_manifest` is true, returns `None`. Otherwise, extracts
    /// the license-file from the package section, handling workspace inheritance if needed.
    /// The path is converted to a string representation.
    fn license_file(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_cargo_manifest {
            return Ok(None);
        }
        let Some(value) = self.ensure_manifest()?.map(|pkg| &pkg.license_file) else {
            return Ok(None);
        };
        let license_file = match value {
            None => return Ok(None),
            Some(Inheritable::Set(value)) => value,
            Some(Inheritable::Inherited) => self
                .ensure_workspace_manifest()?
                .and_then(|template| template.license_file.as_ref())
                .ok_or_else(|| {
                    MetadataError::MissingInheritedValue(String::from(concat!(
                        "workspace.package.license-file",
                    )))
                })?,
        };
        Ok(Some(license_file.display().to_string()))
    }

    /// Returns the package summary from the Cargo.toml manifest.
    ///
    /// Currently always returns `None` as Cargo.toml does not have a summary field.
    /// This could be implemented to return the description field as a fallback.
    fn summary(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(None)
    }

    /// Returns the package documentation URL from the Cargo.toml manifest.
    ///
    /// If `ignore_cargo_manifest` is true, returns `None`. Otherwise, extracts
    /// the documentation from the package section, handling workspace inheritance if needed.
    fn documentation(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_cargo_manifest {
            return Ok(None);
        }
        let Some(value) = self.ensure_manifest()?.map(|pkg| &pkg.documentation) else {
            return Ok(None);
        };
        let documentation = match value {
            None => return Ok(None),
            Some(Inheritable::Set(value)) => value,
            Some(Inheritable::Inherited) => self
                .ensure_workspace_manifest()?
                .and_then(|template| template.documentation.as_ref())
                .ok_or_else(|| {
                    MetadataError::MissingInheritedValue(String::from(concat!(
                        "workspace.package.documentation",
                    )))
                })?,
        };
        Ok(Some(documentation.clone()))
    }

    /// Returns the package repository URL from the Cargo.toml manifest.
    ///
    /// If `ignore_cargo_manifest` is true, returns `None`. Otherwise, extracts
    /// the repository from the package section, handling workspace inheritance if needed.
    fn repository(&mut self) -> Result<Option<String>, Self::Error> {
        if self.ignore_cargo_manifest {
            return Ok(None);
        }
        let Some(value) = self.ensure_manifest()?.map(|pkg| &pkg.repository) else {
            return Ok(None);
        };
        let repository = match value {
            None => return Ok(None),
            Some(Inheritable::Set(value)) => value,
            Some(Inheritable::Inherited) => self
                .ensure_workspace_manifest()?
                .and_then(|template| template.repository.as_ref())
                .ok_or_else(|| {
                    MetadataError::MissingInheritedValue(String::from(concat!(
                        "workspace.package.repository",
                    )))
                })?,
        };
        Ok(Some(repository.clone()))
    }
}
