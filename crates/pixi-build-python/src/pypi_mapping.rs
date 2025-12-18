//! PyPI to conda package name mapping.
//!
//! This module provides functionality to map PyPI package names to their
//! corresponding conda-forge package names using the parselmouth mapping service.

use std::{
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, SystemTime},
};

use indexmap::IndexMap;

use miette::Diagnostic;
use pep508_rs;
use rattler_conda_types::{MatchSpec, PackageName, ParseStrictness, VersionSpec};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Base URL for the PyPI to conda mapping API (without channel suffix).
const MAPPING_BASE_URL: &str = "https://conda-mapping.prefix.dev/pypi-to-conda-v1";

/// Base subdirectory within the cache for storing mapping files.
const CACHE_SUBDIR: &str = "pypi-conda-mapping";

/// Cache validity duration (24 hours).
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Errors that can occur during PyPI to conda mapping.
#[derive(Debug, Error, Diagnostic)]
pub enum MappingError {
    /// Failed to fetch mapping from the API.
    #[error("Failed to fetch conda mapping for '{0}'")]
    FetchError(String, #[source] reqwest::Error),

    /// Failed to parse the mapping response.
    #[error("Failed to parse mapping response for '{0}'")]
    ParseError(String, #[source] serde_json::Error),

    /// Invalid version specifier conversion.
    #[error("Failed to convert version specifier '{0}' to conda format: {1}")]
    VersionConversionError(String, String),

    /// Invalid package name.
    #[error("Invalid conda package name '{0}'")]
    InvalidPackageName(
        String,
        #[source] rattler_conda_types::InvalidPackageNameError,
    ),
}

/// Response format from the PyPI to conda mapping API.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PyPiPackageLookup {
    /// Format version of the response.
    #[allow(dead_code)]
    pub format_version: String,

    /// Channel (e.g., "conda-forge").
    #[allow(dead_code)]
    pub channel: String,

    /// The PyPI package name.
    #[allow(dead_code)]
    pub pypi_name: String,

    /// Mapping of PyPI versions to conda package names.
    /// Key is PyPI version string, value is list of conda package names.
    /// Uses IndexMap to preserve insertion order from the API (latest version is last).
    pub conda_versions: IndexMap<String, Vec<String>>,
}

/// A successfully mapped conda dependency.
#[derive(Debug, Clone)]
pub struct MappedCondaDependency {
    /// The conda package name.
    pub name: PackageName,

    /// Optional version specification.
    pub version_spec: Option<VersionSpec>,
}

impl MappedCondaDependency {
    /// Convert to a conda MatchSpec.
    pub fn to_match_spec(&self) -> MatchSpec {
        MatchSpec {
            name: Some(rattler_conda_types::PackageNameMatcher::Exact(
                self.name.clone(),
            )),
            version: self.version_spec.clone(),
            ..Default::default()
        }
    }
}

/// Mapper for converting PyPI packages to conda packages.
pub struct PyPiToCondaMapper {
    cache_dir: Option<PathBuf>,
    client: reqwest::Client,
    /// The channel name to use for mapping (e.g., "conda-forge").
    channel_name: String,
    /// Inline mappings for testing (bypasses cache and API).
    #[cfg(test)]
    inline_mappings: Option<IndexMap<String, PyPiPackageLookup>>,
}

impl PyPiToCondaMapper {
    /// Create a new mapper with the given cache directory and channel name.
    pub fn new(cache_dir: Option<PathBuf>, channel_name: String) -> Self {
        Self {
            cache_dir,
            client: reqwest::Client::new(),
            channel_name,
            #[cfg(test)]
            inline_mappings: None,
        }
    }

    /// Create a mapper with inline mappings for testing.
    /// This bypasses the cache and API, using only the provided mappings.
    #[cfg(test)]
    pub fn with_inline_mappings(mappings: IndexMap<String, PyPiPackageLookup>) -> Self {
        Self {
            cache_dir: None,
            client: reqwest::Client::new(),
            channel_name: "test".to_string(),
            inline_mappings: Some(mappings),
        }
    }

    /// Normalize a PyPI package name according to PEP 503.
    ///
    /// PyPI package names are case-insensitive and treat `-`, `_`, and `.` as equivalent.
    /// The normalized form uses lowercase with hyphens.
    fn normalize_pypi_name(name: &str) -> String {
        name.to_lowercase().replace(['_', '.'], "-")
    }

    /// Get the cache file path for a normalized package name.
    fn cache_path(&self, normalized_name: &str) -> Option<PathBuf> {
        self.cache_dir.as_ref().map(|dir| {
            dir.join(CACHE_SUBDIR)
                .join(&self.channel_name)
                .join(format!("{}.json", normalized_name))
        })
    }

    /// Check if a cached file is still valid.
    fn is_cache_valid(path: &Path) -> bool {
        if let Ok(metadata) = std::fs::metadata(path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(elapsed) = SystemTime::now().duration_since(modified) {
                    return elapsed < CACHE_TTL;
                }
            }
        }
        false
    }

    /// Read a mapping from the cache.
    fn read_from_cache(&self, normalized_name: &str) -> Option<PyPiPackageLookup> {
        let cache_path = self.cache_path(normalized_name)?;

        if !cache_path.exists() || !Self::is_cache_valid(&cache_path) {
            return None;
        }

        let content = std::fs::read_to_string(&cache_path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Write a mapping to the cache.
    fn write_to_cache(&self, normalized_name: &str, lookup: &PyPiPackageLookup) {
        let Some(cache_path) = self.cache_path(normalized_name) else {
            return;
        };

        if let Some(parent) = cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if let Ok(content) = serde_json::to_string(lookup) {
            let _ = std::fs::write(cache_path, content);
        }
    }

    /// Fetch a mapping from the API.
    async fn fetch_from_api(
        &self,
        normalized_name: &str,
    ) -> Result<Option<PyPiPackageLookup>, MappingError> {
        let url = format!(
            "{}/{}/{}.json",
            MAPPING_BASE_URL, self.channel_name, normalized_name
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| MappingError::FetchError(normalized_name.to_string(), e))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let text = response
            .text()
            .await
            .map_err(|e| MappingError::FetchError(normalized_name.to_string(), e))?;

        let lookup: PyPiPackageLookup = serde_json::from_str(&text)
            .map_err(|e| MappingError::ParseError(normalized_name.to_string(), e))?;

        Ok(Some(lookup))
    }

    /// Get the mapping for a PyPI package, using cache if available.
    pub async fn get_mapping(
        &self,
        pypi_name: &str,
    ) -> Result<Option<PyPiPackageLookup>, MappingError> {
        let normalized_name = Self::normalize_pypi_name(pypi_name);

        // Check inline mappings first (test-only)
        #[cfg(test)]
        if let Some(ref mappings) = self.inline_mappings {
            return Ok(mappings.get(&normalized_name).cloned());
        }

        // Try cache first
        if let Some(cached) = self.read_from_cache(&normalized_name) {
            return Ok(Some(cached));
        }

        // Fetch from API
        let lookup = self.fetch_from_api(&normalized_name).await?;

        // Write to cache if successful
        if let Some(ref lookup) = lookup {
            self.write_to_cache(&normalized_name, lookup);
        }

        Ok(lookup)
    }

    /// Extract conda package names from a lookup.
    ///
    /// Returns the conda package name most similar to the PyPI name.
    /// Prefers exact matches (after normalization), otherwise uses Levenshtein distance.
    fn extract_conda_name(lookup: &PyPiPackageLookup) -> Option<String> {
        // With the current API implementation, the last entry is the latest version.
        // Take the conda names from that version.
        let all_names: Vec<&String> = lookup.conda_versions.values().last()?.iter().collect();

        let normalized_pypi = Self::normalize_pypi_name(&lookup.pypi_name);

        // First check for exact match (after normalization)
        for name in &all_names {
            if Self::normalize_pypi_name(name) == normalized_pypi {
                return Some((*name).clone());
            }
        }

        // Otherwise select the name with smallest Levenshtein distance
        all_names
            .into_iter()
            .min_by_key(|name| {
                strsim::levenshtein(&Self::normalize_pypi_name(name), &normalized_pypi)
            })
            .cloned()
    }

    /// Convert PEP 440 version specifiers to conda VersionSpec.
    ///
    /// This handles common specifiers directly and transforms PEP 440-specific
    /// syntax like `~=` (compatible release) to conda equivalents.
    fn convert_version_specifiers(
        specifiers: &pep508_rs::VersionOrUrl<pep508_rs::VerbatimUrl>,
    ) -> Result<Option<VersionSpec>, MappingError> {
        let pep508_rs::VersionOrUrl::VersionSpecifier(specs) = specifiers else {
            // URL-based dependency, no version constraint
            return Ok(None);
        };

        if specs.is_empty() {
            return Ok(None);
        }

        // Convert specifiers to string and attempt conda parsing
        let spec_str = specs.to_string();

        // Handle PEP 440-specific operators that conda doesn't understand
        let converted = Self::convert_pep440_operators(&spec_str);

        VersionSpec::from_str(&converted, ParseStrictness::Lenient)
            .map(Some)
            .map_err(|e| MappingError::VersionConversionError(spec_str, e.to_string()))
    }

    /// Convert PEP 440-specific operators to conda-compatible equivalents.
    fn convert_pep440_operators(spec_str: &str) -> String {
        let mut result = spec_str.to_string();

        // Handle ~= (compatible release): ~=1.4.2 becomes >=1.4.2,<1.5.0
        // This is a simplified conversion - full implementation would parse versions properly
        if result.contains("~=") {
            // For now, convert ~=X.Y.Z to >=X.Y.Z (lose the upper bound constraint)
            // A more complete implementation would compute the proper upper bound
            result = result.replace("~=", ">=");
            tracing::debug!(
                "Converted compatible release operator ~= to >= (upper bound not enforced)"
            );
        }

        // Handle === (arbitrary equality): ===1.0.0 becomes ==1.0.0
        result = result.replace("===", "==");

        result
    }

    /// Map a list of PEP 508 requirements to conda MatchSpecs.
    ///
    /// Returns a list of successfully mapped dependencies. Unmapped packages
    /// are logged as warnings and skipped.
    pub async fn map_requirements(
        &self,
        requirements: &[pep508_rs::Requirement<pep508_rs::VerbatimUrl>],
    ) -> Result<Vec<MappedCondaDependency>, MappingError> {
        let mut mapped = Vec::new();

        for req in requirements {
            // Skip requirements with environment markers for now
            // A full implementation would evaluate markers against the target platform
            if req.marker != pep508_rs::MarkerTree::default() {
                tracing::debug!(
                    "Skipping dependency '{}' with environment marker: {:?}",
                    req.name,
                    req.marker
                );
                continue;
            }

            // Get the mapping
            let lookup = match self.get_mapping(req.name.as_ref()).await? {
                Some(l) => l,
                None => {
                    tracing::warn!(
                        "PyPI package '{}' has no conda-forge mapping, skipping",
                        req.name
                    );
                    continue;
                }
            };

            // Extract the conda package name
            let conda_name_str = match Self::extract_conda_name(&lookup) {
                Some(n) => n,
                None => {
                    tracing::warn!(
                        "No conda package names found in mapping for '{}', skipping",
                        req.name
                    );
                    continue;
                }
            };

            // Parse conda package name
            let conda_name = PackageName::from_str(&conda_name_str)
                .map_err(|e| MappingError::InvalidPackageName(conda_name_str.clone(), e))?;

            // Convert version specifiers
            let version_spec = if let Some(ref version_or_url) = req.version_or_url {
                match Self::convert_version_specifiers(version_or_url) {
                    Ok(spec) => spec,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to convert version specifier for '{}': {}, using unconstrained version",
                            req.name,
                            e
                        );
                        None
                    }
                }
            } else {
                None
            };

            mapped.push(MappedCondaDependency {
                name: conda_name,
                version_spec,
            });
        }

        Ok(mapped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_pypi_name() {
        assert_eq!(
            PyPiToCondaMapper::normalize_pypi_name("Requests"),
            "requests"
        );
        assert_eq!(
            PyPiToCondaMapper::normalize_pypi_name("my_package"),
            "my-package"
        );
        assert_eq!(
            PyPiToCondaMapper::normalize_pypi_name("My.Package"),
            "my-package"
        );
        assert_eq!(
            PyPiToCondaMapper::normalize_pypi_name("SOME_PACKAGE.NAME"),
            "some-package-name"
        );
    }

    #[test]
    fn test_convert_pep440_operators() {
        assert_eq!(
            PyPiToCondaMapper::convert_pep440_operators(">=1.0,<2.0"),
            ">=1.0,<2.0"
        );
        assert_eq!(
            PyPiToCondaMapper::convert_pep440_operators("===1.0.0"),
            "==1.0.0"
        );
        assert_eq!(
            PyPiToCondaMapper::convert_pep440_operators("~=1.4.2"),
            ">=1.4.2"
        );
    }

    #[test]
    fn test_extract_conda_name() {
        let lookup = PyPiPackageLookup {
            format_version: "1".to_string(),
            channel: "conda-forge".to_string(),
            pypi_name: "requests".to_string(),
            conda_versions: IndexMap::from([
                ("2.31.0".to_string(), vec!["requests".to_string()]),
                ("2.32.0".to_string(), vec!["requests".to_string()]),
            ]),
        };

        assert_eq!(
            PyPiToCondaMapper::extract_conda_name(&lookup),
            Some("requests".to_string())
        );
    }

    #[test]
    fn test_extract_conda_name_empty() {
        let lookup = PyPiPackageLookup {
            format_version: "1".to_string(),
            channel: "conda-forge".to_string(),
            pypi_name: "unknown".to_string(),
            conda_versions: IndexMap::new(),
        };

        assert_eq!(PyPiToCondaMapper::extract_conda_name(&lookup), None);
    }

    #[test]
    fn test_extract_conda_name_prefers_similar_name() {
        // When multiple conda packages exist, prefer the one most similar to pypi_name
        let lookup = PyPiPackageLookup {
            format_version: "1.0".to_string(),
            channel: "conda-forge".to_string(),
            pypi_name: "jinja2".to_string(),
            conda_versions: IndexMap::from([(
                "3.1.3".to_string(),
                vec!["jinja2".to_string(), "jupyter-sphinx".to_string()],
            )]),
        };

        assert_eq!(
            PyPiToCondaMapper::extract_conda_name(&lookup),
            Some("jinja2".to_string())
        );
    }

    #[test]
    fn test_extract_conda_name_uses_levenshtein_when_no_exact_match() {
        // When no exact match exists, use Levenshtein distance
        let lookup = PyPiPackageLookup {
            format_version: "1.0".to_string(),
            channel: "conda-forge".to_string(),
            pypi_name: "some-package".to_string(),
            conda_versions: IndexMap::from([(
                "1.0.0".to_string(),
                vec!["some-pkg".to_string(), "totally-different".to_string()],
            )]),
        };

        // "some-pkg" is closer to "some-package" than "totally-different"
        assert_eq!(
            PyPiToCondaMapper::extract_conda_name(&lookup),
            Some("some-pkg".to_string())
        );
    }

    #[tokio::test]
    async fn test_map_requirements_with_inline_mappings() {
        let mappings = IndexMap::from([
            (
                "requests".to_string(),
                PyPiPackageLookup {
                    format_version: "1".to_string(),
                    channel: "conda-forge".to_string(),
                    pypi_name: "requests".to_string(),
                    conda_versions: IndexMap::from([(
                        "2.31.0".to_string(),
                        vec!["requests".to_string()],
                    )]),
                },
            ),
            (
                "flask".to_string(),
                PyPiPackageLookup {
                    format_version: "1".to_string(),
                    channel: "conda-forge".to_string(),
                    pypi_name: "flask".to_string(),
                    conda_versions: IndexMap::from([(
                        "2.0.0".to_string(),
                        vec!["flask".to_string()],
                    )]),
                },
            ),
        ]);

        let mapper = PyPiToCondaMapper::with_inline_mappings(mappings);

        let requirements = vec![
            pep508_rs::Requirement::from_str("requests>=2.0").unwrap(),
            pep508_rs::Requirement::from_str("flask").unwrap(),
        ];

        let mapped = mapper.map_requirements(&requirements).await.unwrap();

        assert_eq!(mapped.len(), 2);
        assert_eq!(mapped[0].name.as_normalized(), "requests");
        assert_eq!(
            mapped[0].version_spec.as_ref().unwrap().to_string(),
            ">=2.0"
        );
        assert_eq!(mapped[1].name.as_normalized(), "flask");
        assert!(mapped[1].version_spec.is_none());
    }
}
