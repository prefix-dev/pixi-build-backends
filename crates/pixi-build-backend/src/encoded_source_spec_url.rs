use std::{collections::HashMap, str::FromStr};

use pixi_build_types::{GitReferenceV1, GitSpecV1, SourcePackageSpecV1, UrlSpecV1};
use url::Url;

/// An internal type that supports converting a source dependency into a valid
/// URL and back.
///
/// This type is only used internally, it is not serialized. Therefore,
/// stability of how the URL is encoded is not important.
pub(crate) struct EncodedSourceSpecUrl(Url);

impl From<EncodedSourceSpecUrl> for Url {
    fn from(value: EncodedSourceSpecUrl) -> Self {
        value.0
    }
}

impl From<Url> for EncodedSourceSpecUrl {
    fn from(url: Url) -> Self {
        // Ensure the URL is a file URL
        assert_eq!(url.scheme(), "source", "URL must be a file URL");
        Self(url)
    }
}

impl From<EncodedSourceSpecUrl> for SourcePackageSpecV1 {
    fn from(value: EncodedSourceSpecUrl) -> Self {
        let url = value.0;
        assert_eq!(url.scheme(), "source", "URL must be a file URL");
        let mut pairs: HashMap<_, _> = url.query_pairs().collect();
        if let Some(path) = pairs.remove("path") {
            SourcePackageSpecV1::Path(pixi_build_types::PathSpecV1 {
                path: path.into_owned(),
            })
        } else if let Some(url) = pairs.remove("url") {
            let url = Url::from_str(&url).expect("must be a valid URL");
            let md5 = pairs
                .remove("md5")
                .and_then(|s| rattler_digest::parse_digest_from_hex::<rattler_digest::Md5>(&s));
            let sha256 = pairs
                .remove("sha256")
                .and_then(|s| rattler_digest::parse_digest_from_hex::<rattler_digest::Sha256>(&s));
            SourcePackageSpecV1::Url(UrlSpecV1 { url, md5, sha256 })
        } else if let Some(git) = pairs.remove("git") {
            let git_url = Url::from_str(&git).expect("must be a valid URL");
            let rev = if let Some(rev) = pairs.remove("rev") {
                Some(GitReferenceV1::Rev(rev.into_owned()))
            } else if let Some(branch) = pairs.remove("branch") {
                Some(GitReferenceV1::Branch(branch.into_owned()))
            } else {
                pairs
                    .remove("tag")
                    .map(|tag| GitReferenceV1::Tag(tag.into_owned()))
            };

            let subdirectory = pairs.remove("subdirectory").map(|s| s.into_owned());
            SourcePackageSpecV1::Git(GitSpecV1 {
                git: git_url,
                rev,
                subdirectory,
            })
        } else {
            panic!("URL must contain either 'path', 'url', or 'git' query parameters");
        }
    }
}

impl From<SourcePackageSpecV1> for EncodedSourceSpecUrl {
    fn from(value: SourcePackageSpecV1) -> Self {
        let mut url = Url::from_str("source://").expect("must be a valid URL");
        let mut query_pairs = url.query_pairs_mut();
        match value {
            SourcePackageSpecV1::Url(url) => {
                query_pairs.append_pair("url", url.url.as_str());
                if let Some(md5) = &url.md5 {
                    query_pairs.append_pair("md5", &format!("{md5:x}"));
                }
                if let Some(sha256) = &url.sha256 {
                    query_pairs.append_pair("sha256", &format!("{sha256:x}"));
                }
            }
            SourcePackageSpecV1::Git(git) => {
                query_pairs.append_pair("git", git.git.as_str());
                if let Some(subdirectory) = &git.subdirectory {
                    query_pairs.append_pair("subdirectory", subdirectory);
                }
                match &git.rev {
                    Some(GitReferenceV1::Branch(branch)) => {
                        query_pairs.append_pair("branch", branch);
                    }
                    Some(GitReferenceV1::Rev(rev)) => {
                        query_pairs.append_pair("rev", rev);
                    }
                    Some(GitReferenceV1::Tag(tag)) => {
                        query_pairs.append_pair("tag", tag);
                    }
                    _ => {}
                }
            }
            SourcePackageSpecV1::Path(path) => {
                query_pairs.append_pair("path", &path.path);
            }
        };
        drop(query_pairs);
        Self(url)
    }
}

#[cfg(test)]
mod test {
    use rattler_digest::{Md5, Sha256};

    use super::*;

    #[test]
    fn test_conversion() {
        for spec in [
            SourcePackageSpecV1::Path(pixi_build_types::PathSpecV1 {
                path: "..\\test\\path".into(),
            }),
            SourcePackageSpecV1::Path(pixi_build_types::PathSpecV1 {
                path: "../test/path".into(),
            }),
            SourcePackageSpecV1::Path(pixi_build_types::PathSpecV1 {
                path: "test/path".into(),
            }),
            SourcePackageSpecV1::Path(pixi_build_types::PathSpecV1 {
                path: "/absolute/test/path".into(),
            }),
            SourcePackageSpecV1::Path(pixi_build_types::PathSpecV1 {
                path: "C://absolute/win/path".into(),
            }),
            SourcePackageSpecV1::Git(pixi_build_types::GitSpecV1 {
                git: "https://github.com/some/repo.git".parse().unwrap(),
                rev: Some(GitReferenceV1::Rev("1234567890abcdef".into())),
                subdirectory: Some("subdir".into()),
            }),
            SourcePackageSpecV1::Url(pixi_build_types::UrlSpecV1 {
                url: "https://example.com/some/file.tar.gz".parse().unwrap(),
                md5: Some(
                    rattler_digest::parse_digest_from_hex::<Md5>(
                        "d41d8cd98f00b204e9800998ecf8427e",
                    )
                    .unwrap(),
                ),
                sha256: Some(
                    rattler_digest::parse_digest_from_hex::<Sha256>(
                        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                    )
                    .unwrap(),
                ),
            }),
        ] {
            let url: EncodedSourceSpecUrl = spec.clone().into();
            let converted_spec: SourcePackageSpecV1 = url.into();
            assert_eq!(spec, converted_spec);
        }
    }
}
