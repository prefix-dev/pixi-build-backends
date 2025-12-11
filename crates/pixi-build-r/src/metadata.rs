use std::{collections::BTreeSet, path::PathBuf};

use miette::Diagnostic;
use once_cell::unsync::OnceCell;
use pixi_build_backend::generated_recipe::MetadataProvider;
use rattler_conda_types::{ParseVersionError, Version};
use std::str::FromStr;

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum MetadataError {
    // #[error("failed to parse DESCRIPTION file: {0}")]
    // ParseDescription(String),
    #[error("failed to parse version from DESCRIPTION: {0}")]
    ParseVersion(#[from] ParseVersionError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Parsed DESCRIPTION file data
#[derive(Debug, Clone)]
struct DescriptionData {
    package: Option<String>,
    version: Option<String>,
    title: Option<String>,
    description: Option<String>,
    license: Option<String>,
    url: Option<String>,
    bug_reports: Option<String>,
    maintainer: Option<String>,
    linking_to: Option<String>,
    depends: Option<String>,
    imports: Option<String>,
}

impl Default for DescriptionData {
    fn default() -> Self {
        Self {
            package: None,
            version: None,
            title: None,
            description: None,
            license: None,
            url: None,
            bug_reports: None,
            maintainer: None,
            linking_to: None,
            depends: None,
            imports: None,
        }
    }
}

/// MetadataProvider implementation for R DESCRIPTION files
pub struct DescriptionMetadataProvider {
    manifest_root: PathBuf,
    description_data: OnceCell<DescriptionData>,
}

impl DescriptionMetadataProvider {
    pub fn new(manifest_root: impl Into<PathBuf>) -> Self {
        Self {
            manifest_root: manifest_root.into(),
            description_data: OnceCell::default(),
        }
    }

    /// Parse DESCRIPTION file in DCF (Debian Control File) format
    fn parse_description(content: &str) -> Result<DescriptionData, MetadataError> {
        let mut data = DescriptionData::default();

        let mut current_key: Option<String> = None;
        let mut current_value = String::new();

        for line in content.lines() {
            if line.is_empty() {
                continue;
            }

            // Continuation line (starts with whitespace)
            if line.starts_with(char::is_whitespace) {
                if !current_value.is_empty() {
                    current_value.push(' ');
                }
                current_value.push_str(line.trim());
            } else if let Some(colon_pos) = line.find(':') {
                // Save previous key-value pair
                if let Some(key) = current_key.take() {
                    Self::store_field(&mut data, &key, &current_value);
                }

                // Start new key-value pair
                let key = line[..colon_pos].trim().to_string();
                current_value = line[colon_pos + 1..].trim().to_string();
                current_key = Some(key);
            }
        }

        // Store final key-value pair
        if let Some(key) = current_key {
            Self::store_field(&mut data, &key, &current_value);
        }

        Ok(data)
    }

    fn store_field(data: &mut DescriptionData, key: &str, value: &str) {
        let value = value.trim().to_string();
        if value.is_empty() {
            return;
        }

        match key {
            "Package" => data.package = Some(value),
            "Version" => data.version = Some(value),
            "Title" => data.title = Some(value),
            "Description" => data.description = Some(value),
            "License" => data.license = Some(value),
            "URL" => data.url = Some(value),
            "BugReports" => data.bug_reports = Some(value),
            "Maintainer" => data.maintainer = Some(value),
            "LinkingTo" => data.linking_to = Some(value),
            "Depends" => data.depends = Some(value),
            "Imports" => data.imports = Some(value),
            _ => {}
        }
    }

    fn ensure_data(&self) -> Result<&DescriptionData, MetadataError> {
        self.description_data.get_or_try_init(|| {
            let description_path = self.manifest_root.join("DESCRIPTION");
            let content = fs_err::read_to_string(&description_path)?;
            Self::parse_description(&content)
        })
    }

    /// Check if package has native code by looking for src/ directory
    pub fn has_native_code(&self) -> bool {
        let src_dir = self.manifest_root.join("src");
        src_dir.exists() && src_dir.is_dir()
    }

    /// Check if package has LinkingTo dependencies (indicates C++ code)
    pub fn has_linking_to(&self) -> Result<bool, MetadataError> {
        Ok(self.ensure_data()?.linking_to.is_some())
    }

    /// Returns input globs for R package files
    pub fn input_globs(&self) -> BTreeSet<String> {
        let mut globs = BTreeSet::new();

        if self.description_data.get().is_some() {
            globs.insert("DESCRIPTION".to_string());
        }

        globs
    }
}

impl MetadataProvider for DescriptionMetadataProvider {
    type Error = MetadataError;

    fn name(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(self.ensure_data()?.package.clone())
    }

    fn version(&mut self) -> Result<Option<Version>, Self::Error> {
        let data = self.ensure_data()?;
        match &data.version {
            Some(v) => Ok(Some(Version::from_str(v)?)),
            None => Ok(None),
        }
    }

    fn description(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(self.ensure_data()?.description.clone())
    }

    fn homepage(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(self.ensure_data()?.url.clone())
    }

    fn license(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(self.ensure_data()?.license.clone())
    }

    fn summary(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(self.ensure_data()?.title.clone())
    }

    fn repository(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(self.ensure_data()?.bug_reports.clone())
    }

    fn license_file(&mut self) -> Result<Option<String>, Self::Error> {
        // R packages typically don't specify license files in DESCRIPTION
        Ok(None)
    }

    fn documentation(&mut self) -> Result<Option<String>, Self::Error> {
        // R packages don't typically have separate docs URL
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_description(content: &str) -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let desc_path = temp_dir.path().join("DESCRIPTION");
        fs::write(desc_path, content).unwrap();
        temp_dir
    }

    #[test]
    fn test_parse_basic_description() {
        let content = r#"Package: testpkg
Version: 1.0.0
Title: Test Package
Description: A test package for testing
License: GPL-3
"#;
        let temp_dir = create_test_description(content);
        let mut provider = DescriptionMetadataProvider::new(temp_dir.path());

        assert_eq!(provider.name().unwrap(), Some("testpkg".to_string()));
        assert_eq!(
            provider.version().unwrap().unwrap().to_string(),
            "1.0.0"
        );
        assert_eq!(provider.license().unwrap(), Some("GPL-3".to_string()));
        assert_eq!(
            provider.summary().unwrap(),
            Some("Test Package".to_string())
        );
        assert_eq!(
            provider.description().unwrap(),
            Some("A test package for testing".to_string())
        );
    }

    #[test]
    fn test_multiline_description() {
        let content = r#"Package: testpkg
Version: 1.0.0
Description: This is a long description
    that spans multiple lines
    with continuation.
License: MIT
"#;
        let temp_dir = create_test_description(content);
        let mut provider = DescriptionMetadataProvider::new(temp_dir.path());

        let desc = provider.description().unwrap().unwrap();
        assert!(desc.contains("long description"));
        assert!(desc.contains("multiple lines"));
        assert!(desc.contains("continuation"));
    }

    #[test]
    fn test_all_fields() {
        let content = r#"Package: fullpkg
Version: 2.1.3
Title: Full Package Example
Description: A comprehensive example
    with all fields populated.
License: GPL-3
URL: https://github.com/user/fullpkg
BugReports: https://github.com/user/fullpkg/issues
Maintainer: John Doe <john@example.com>
LinkingTo: Rcpp, RcppArmadillo
Depends: R (>= 3.5.0), dplyr
Imports: ggplot2, tidyr
"#;
        let temp_dir = create_test_description(content);
        let mut provider = DescriptionMetadataProvider::new(temp_dir.path());

        assert_eq!(provider.name().unwrap(), Some("fullpkg".to_string()));
        assert_eq!(
            provider.version().unwrap().unwrap().to_string(),
            "2.1.3"
        );
        assert_eq!(
            provider.summary().unwrap(),
            Some("Full Package Example".to_string())
        );
        assert_eq!(provider.license().unwrap(), Some("GPL-3".to_string()));
        assert_eq!(
            provider.homepage().unwrap(),
            Some("https://github.com/user/fullpkg".to_string())
        );
        assert_eq!(
            provider.repository().unwrap(),
            Some("https://github.com/user/fullpkg/issues".to_string())
        );
        assert!(provider.has_linking_to().unwrap());
    }

    #[test]
    fn test_native_code_detection() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(
            temp_dir.path().join("DESCRIPTION"),
            "Package: test\nVersion: 1.0.0",
        )
        .unwrap();
        fs::create_dir(temp_dir.path().join("src")).unwrap();

        let provider = DescriptionMetadataProvider::new(temp_dir.path());
        assert!(provider.has_native_code());
    }

    #[test]
    fn test_no_native_code() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(
            temp_dir.path().join("DESCRIPTION"),
            "Package: test\nVersion: 1.0.0",
        )
        .unwrap();

        let provider = DescriptionMetadataProvider::new(temp_dir.path());
        assert!(!provider.has_native_code());
    }

    #[test]
    fn test_linking_to_detection() {
        let content = r#"Package: testpkg
Version: 1.0.0
LinkingTo: Rcpp
"#;
        let temp_dir = create_test_description(content);
        let provider = DescriptionMetadataProvider::new(temp_dir.path());

        assert!(provider.has_linking_to().unwrap());
    }

    #[test]
    fn test_no_linking_to() {
        let content = r#"Package: testpkg
Version: 1.0.0
Title: Pure R Package
"#;
        let temp_dir = create_test_description(content);
        let provider = DescriptionMetadataProvider::new(temp_dir.path());

        assert!(!provider.has_linking_to().unwrap());
    }

    #[test]
    fn test_missing_fields() {
        let content = r#"Package: minimalpkg
Version: 0.1.0
"#;
        let temp_dir = create_test_description(content);
        let mut provider = DescriptionMetadataProvider::new(temp_dir.path());

        assert_eq!(provider.name().unwrap(), Some("minimalpkg".to_string()));
        assert_eq!(
            provider.version().unwrap().unwrap().to_string(),
            "0.1.0"
        );
        assert_eq!(provider.description().unwrap(), None);
        assert_eq!(provider.license().unwrap(), None);
        assert_eq!(provider.homepage().unwrap(), None);
    }

    #[test]
    fn test_input_globs() {
        let content = r#"Package: testpkg
Version: 1.0.0
"#;
        let temp_dir = create_test_description(content);
        let provider = DescriptionMetadataProvider::new(temp_dir.path());

        // Force data loading by accessing it
        let _data = provider.ensure_data().unwrap();

        let globs = provider.input_globs();
        assert!(globs.contains("DESCRIPTION"));
    }
}
