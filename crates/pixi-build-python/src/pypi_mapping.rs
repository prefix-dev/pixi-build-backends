//! PyPI to conda package name mapping.
//!
//! This module provides functionality to map PyPI package names to their
//! corresponding conda-forge package names using the parselmouth mapping service.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, SystemTime},
};

use indexmap::IndexMap;

use miette::Diagnostic;
use rattler_conda_types::{ChannelUrl, MatchSpec, PackageName, ParseStrictness, VersionSpec};
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
    #[error("failed to fetch conda mapping for '{0}'")]
    FetchError(String, #[source] reqwest::Error),

    /// Failed to parse the mapping response.
    #[error("failed to parse mapping response for '{0}'")]
    ParseError(String, #[source] serde_json::Error),

    /// Invalid version specifier conversion.
    #[error("failed to convert version specifier '{0}' to conda format: {1}")]
    VersionConversionError(String, String),

    /// Invalid package name.
    #[error("invalid conda package name '{0}'")]
    InvalidPackageName(
        String,
        #[source] rattler_conda_types::InvalidPackageNameError,
    ),
}

/// Response format from the PyPI to conda mapping API.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PyPiPackageLookup {
    /// Format version of the response.
    pub format_version: String,

    /// Channel (e.g., "conda-forge").
    pub channel: String,

    /// The PyPI package name.
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
        if let Ok(metadata) = std::fs::metadata(path)
            && let Ok(modified) = metadata.modified()
            && let Ok(elapsed) = SystemTime::now().duration_since(modified)
        {
            return elapsed < CACHE_TTL;
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
        pypi_name: &str,
    ) -> Result<Option<PyPiPackageLookup>, MappingError> {
        let url = format!(
            "{}/{}/{}.json",
            MAPPING_BASE_URL, self.channel_name, pypi_name
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| MappingError::FetchError(pypi_name.to_string(), e))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let text = response
            .text()
            .await
            .map_err(|e| MappingError::FetchError(pypi_name.to_string(), e))?;

        let lookup: PyPiPackageLookup = serde_json::from_str(&text)
            .map_err(|e| MappingError::ParseError(pypi_name.to_string(), e))?;

        Ok(Some(lookup))
    }

    /// Get the mapping for a PyPI package, using cache if available.
    pub async fn get_mapping(
        &self,
        pypi_name: &str,
    ) -> Result<Option<PyPiPackageLookup>, MappingError> {
        // Check inline mappings first (test-only)
        #[cfg(test)]
        if let Some(ref mappings) = self.inline_mappings {
            return Ok(mappings.get(pypi_name).cloned());
        }

        // Try cache
        if let Some(cached) = self.read_from_cache(pypi_name) {
            return Ok(Some(cached));
        }

        // Fetch from API
        let lookup = self.fetch_from_api(pypi_name).await?;

        // Write to cache if successful
        if let Some(ref lookup) = lookup {
            self.write_to_cache(pypi_name, lookup);
        }

        Ok(lookup)
    }

    /// Extract conda package names from a lookup.
    ///
    /// Returns the conda package name most similar to the PyPI name.
    /// Prefers exact matches, otherwise uses Levenshtein distance.
    fn extract_conda_name(lookup: &PyPiPackageLookup) -> Option<String> {
        // With the current API implementation, the last entry is the latest version.
        // Take the conda names from that version.
        let all_names: Vec<&String> = lookup.conda_versions.values().last()?.iter().collect();

        let pypi_name = &lookup.pypi_name;

        // First check for exact match
        for name in &all_names {
            if name == &pypi_name {
                return Some(name.to_string());
            }
        }

        // Otherwise select the name with smallest Levenshtein distance
        all_names
            .into_iter()
            .min_by_key(|name| strsim::levenshtein(name, pypi_name))
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

        // Handle PEP 440-specific operators that conda doesn't understand
        let spec_str = specs.to_string();
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

/// Filter mapped PyPI dependencies, returning only those not already specified
/// in Pixi's run dependencies.
///
/// This implements the merging behavior where Pixi dependencies take precedence
/// over inferred pyproject.toml dependencies. Dependencies not specified in
/// `skip_packages` are returned as MatchSpecs ready to be added to requirements.
pub fn filter_mapped_pypi_deps(
    mapped_deps: &[MappedCondaDependency],
    skip_packages: &HashSet<pixi_build_types::SourcePackageName>,
) -> Vec<MatchSpec> {
    mapped_deps
        .iter()
        .filter(|dep| {
            let pkg_name = pixi_build_types::SourcePackageName::from(dep.name.as_normalized());
            !skip_packages.contains(&pkg_name)
        })
        .map(|dep| dep.to_match_spec())
        .collect()
}

/// Extract the channel name from a channel URL.
///
/// Returns the last path segment (e.g., "conda-forge" from
/// "https://prefix.dev/conda-forge").
pub fn extract_channel_name(channel: &ChannelUrl) -> Option<&str> {
    channel.as_str().trim_end_matches('/').rsplit('/').next()
}

/// Map PyPI requirements to conda dependencies using the first channel that provides a valid mapping.
///
/// Tries each channel in order and returns the mapped dependencies from the first
/// channel that successfully maps at least one dependency. Returns an empty Vec
/// if no channel provides a mapping.
///
/// The `context` parameter is used for logging (e.g., "project dependencies" or
/// "build-system requirements").
pub async fn map_requirements_with_channels(
    requirements: &[pep508_rs::Requirement<pep508_rs::VerbatimUrl>],
    channels: &[ChannelUrl],
    cache_dir: &Option<PathBuf>,
    context: &str,
) -> Vec<MappedCondaDependency> {
    for channel in channels {
        if let Some(channel_name) = extract_channel_name(channel) {
            let mapper = PyPiToCondaMapper::new(cache_dir.clone(), channel_name.to_string());
            match mapper.map_requirements(requirements).await {
                Ok(deps) if !deps.is_empty() => {
                    tracing::debug!(
                        "Using PyPI-to-conda mapping for {} from channel '{}'",
                        context,
                        channel_name
                    );
                    return deps;
                }
                Ok(_) => {
                    tracing::debug!(
                        "No PyPI-to-conda mapping found for {} in channel '{}'",
                        context,
                        channel_name
                    );
                }
                Err(e) => {
                    tracing::info!(
                        "Failed to get PyPI-to-conda mapping for {} in channel '{}': {}",
                        context,
                        channel_name,
                        e
                    );
                }
            }
        }
    }
    Vec::new()
}

/// Build tools that require specific compilers.
///
/// Maps PyPI package names to the compilers they require. This is used to
/// automatically detect compilers from `build-system.requires` in pyproject.toml.
const BUILD_TOOL_COMPILER_MAPPINGS: &[(&str, &[&str])] =
    &[("maturin", &["rust"]), ("setuptools-rust", &["rust"])];

/// Detect compilers required by build tools in `build-system.requires`.
///
/// Examines the list of PEP 508 requirements and returns any compilers that
/// should be automatically added based on the detected build tools.
pub fn detect_compilers_from_build_requirements(
    requirements: &[pep508_rs::Requirement<pep508_rs::VerbatimUrl>],
) -> Vec<String> {
    let mut detected_compilers = HashSet::new();

    for req in requirements {
        let package_name = req.name.as_ref();

        for (tool_name, compilers) in BUILD_TOOL_COMPILER_MAPPINGS {
            if package_name == *tool_name {
                detected_compilers.extend(compilers.iter().map(|s| s.to_string()));
            }
        }
    }

    detected_compilers.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn make_mapped_dep(name: &str, version_spec: Option<&str>) -> MappedCondaDependency {
        MappedCondaDependency {
            name: PackageName::from_str(name).unwrap(),
            version_spec: version_spec
                .map(|s| VersionSpec::from_str(s, ParseStrictness::Lenient).unwrap()),
        }
    }

    #[test]
    fn test_filter_mapped_pypi_deps_without_pixi_deps() {
        // When no Pixi deps are specified, all mapped deps should pass through
        let mapped_deps = vec![
            make_mapped_dep("requests", Some(">=2.0")),
            make_mapped_dep("flask", None),
        ];

        let skip_packages: HashSet<pixi_build_types::SourcePackageName> = HashSet::new();

        let result = filter_mapped_pypi_deps(&mapped_deps, &skip_packages);

        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|r| r.to_string().contains("requests")));
        assert!(result.iter().any(|r| r.to_string().contains("flask")));
    }

    #[test]
    fn test_filter_mapped_pypi_deps_override_but_others_preserved() {
        // When Pixi specifies some deps, those should be filtered out
        // but other deps should still pass through
        let mapped_deps = vec![
            make_mapped_dep("requests", Some(">=2.0")),
            make_mapped_dep("flask", Some(">=1.0")),
            make_mapped_dep("numpy", None),
        ];

        // Pixi specifies "requests" - it should be filtered out
        let skip_packages: HashSet<pixi_build_types::SourcePackageName> =
            HashSet::from([pixi_build_types::SourcePackageName::from("requests")]);

        let result = filter_mapped_pypi_deps(&mapped_deps, &skip_packages);

        // requests should NOT be in result (filtered by Pixi override)
        // flask and numpy should be in result
        assert_eq!(result.len(), 2);
        assert!(!result.iter().any(|r| r.to_string().contains("requests")));
        assert!(result.iter().any(|r| r.to_string().contains("flask")));
        assert!(result.iter().any(|r| r.to_string().contains("numpy")));
    }

    #[test]
    fn test_filter_mapped_pypi_deps_all_filtered_when_all_in_pixi() {
        // When all mapped deps are already in Pixi, nothing should pass through
        let mapped_deps = vec![
            make_mapped_dep("requests", Some(">=2.0")),
            make_mapped_dep("flask", None),
        ];

        let skip_packages: HashSet<pixi_build_types::SourcePackageName> = HashSet::from([
            pixi_build_types::SourcePackageName::from("requests"),
            pixi_build_types::SourcePackageName::from("flask"),
        ]);

        let result = filter_mapped_pypi_deps(&mapped_deps, &skip_packages);

        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_channel_name() {
        use url::Url;

        // Test extracting channel name from various URL formats
        let url1 = ChannelUrl::from(Url::parse("https://prefix.dev/conda-forge").unwrap());
        assert_eq!(extract_channel_name(&url1), Some("conda-forge"));

        let url2 = ChannelUrl::from(Url::parse("https://conda.anaconda.org/conda-forge/").unwrap());
        assert_eq!(extract_channel_name(&url2), Some("conda-forge"));

        let url3 = ChannelUrl::from(Url::parse("https://example.com/my-channel").unwrap());
        assert_eq!(extract_channel_name(&url3), Some("my-channel"));
    }

    #[test]
    fn test_detect_compilers_maturin() {
        let requirements = vec![pep508_rs::Requirement::from_str("maturin>=1.0,<2.0").unwrap()];

        let compilers = detect_compilers_from_build_requirements(&requirements);

        assert_eq!(compilers, vec!["rust"]);
    }

    #[test]
    fn test_detect_compilers_setuptools_rust() {
        let requirements = vec![pep508_rs::Requirement::from_str("setuptools-rust>=1.0").unwrap()];

        let compilers = detect_compilers_from_build_requirements(&requirements);

        assert_eq!(compilers, vec!["rust"]);
    }

    #[test]
    fn test_detect_compilers_no_special_tools() {
        let requirements = vec![
            pep508_rs::Requirement::from_str("setuptools>=42").unwrap(),
            pep508_rs::Requirement::from_str("wheel").unwrap(),
        ];

        let compilers = detect_compilers_from_build_requirements(&requirements);

        assert!(compilers.is_empty());
    }

    #[test]
    fn test_detect_compilers_deduplicates() {
        // Both maturin and setuptools-rust require rust - should only appear once
        let requirements = vec![
            pep508_rs::Requirement::from_str("maturin>=1.0").unwrap(),
            pep508_rs::Requirement::from_str("setuptools-rust>=1.0").unwrap(),
        ];

        let compilers = detect_compilers_from_build_requirements(&requirements);

        assert_eq!(compilers, vec!["rust"]);
    }
}
