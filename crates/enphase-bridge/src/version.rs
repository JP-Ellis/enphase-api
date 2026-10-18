//! CLI version information.
//!
//! This module relies on `build.rs` to provide version information during
//! compilation, as exposed through various `CARGO_BUILD_*` environment
//! variables.

use core::fmt;

use serde::Serialize;

/// Version information.
///
/// This consists of a version tuple parsed from git tags, along with optional
/// commit information if available.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Version {
    /// The version tuple.
    ///
    /// This is a tuple of three integers representing the major, minor, and
    /// patch version numbers.
    pub version: (u32, u32, u32),

    /// Git commit information.
    ///
    /// This may be `None` if the build was not done from a git repository.
    pub commit_info: Option<CommitInfo>,
}

impl Default for Version {
    /// Retrieve version information from build-time environment variables.
    ///
    /// This will use the `CARGO_BUILD_TAG` environment variable to determine
    /// the version, falling back to `CARGO_PKG_VERSION` if not available (which
    /// is set by Cargo based on `Cargo.toml`).
    ///
    /// The version is expected to be a tag of the form `enphase-bridge/v?X.Y.Z`
    /// or for the Cargo package version fallback, simply `X.Y.Z`.
    ///
    /// # Panics
    ///
    /// This function will panic if the version information cannot be parsed
    /// (due to an invalid format).
    #[expect(clippy::expect_used, reason = "Version should be set at build time.")]
    fn default() -> Self {
        // Try to get version from git tag, fallback to CARGO_PKG_VERSION
        let mut version_str = option_env!("CARGO_BUILD_TAG").unwrap_or(env!("CARGO_PKG_VERSION"));

        // Strip the leading `enphase-bridge/` prefix, and `v` if present
        if let Some(stripped) = version_str.strip_prefix("enphase-bridge/") {
            version_str = stripped;
        }
        if let Some(stripped) = version_str.strip_prefix("v") {
            version_str = stripped;
        }

        let mut parts = version_str
            .split('.')
            .map(|s| s.parse::<u32>().expect("Failed to parse version segment"));
        let mut next = || parts.next().expect("Version string is missing segments");

        let version = (next(), next(), next());

        Self {
            version,
            commit_info: CommitInfo::from_build_env(),
        }
    }
}

impl fmt::Display for Version {
    /// Format version as: `X.Y.Z[.devN] (hash date)`.
    ///
    /// Examples:
    /// - `1.2.3 (abcdef0 2025-01-15)` - at a tag
    /// - `1.2.3.dev5 (abcdef0 2025-01-15)` - 5 commits after tag
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}",
            self.version.0, self.version.1, self.version.2
        )?;

        if let Some(info) = &self.commit_info {
            let hash = &info.short_commit_hash;
            let date = &info.commit_date;
            match info.tag_distance {
                Some(d) if d > 0 => write!(f, ".dev{d} ({hash} {date})")?,
                _ => write!(f, " ({hash} {date})")?,
            }
        }

        Ok(())
    }
}

impl PartialOrd for Version {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    #[inline]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.version.cmp(&other.version)
    }
}

impl From<Version> for clap::builder::Str {
    #[inline]
    fn from(v: Version) -> Self {
        clap::builder::Str::from(v.to_string())
    }
}

/// Information about the git repository where the CLI was built.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitInfo {
    /// Short commit hash.
    pub short_commit_hash: String,
    /// Full commit hash.
    pub commit_hash: String,
    /// Commit date in YYYY-MM-DD format.
    pub commit_date: String,
    /// Last git tag.
    ///
    /// This may not be present if there are no tags, or if the build is from
    /// a shallow clone.
    pub tag: Option<String>,
    /// Number of commits since the last tag.
    ///
    /// As with `tag`, this may not always be present.
    pub tag_distance: Option<u32>,
}

impl CommitInfo {
    /// Read the environment variables set by build.rs and return a new
    /// `CommitInfo` instance.
    ///
    /// If any of the required environment variables are missing, this function
    /// will return `None`.
    pub(crate) fn from_build_env() -> Option<Self> {
        // At a minimum, we need the commit hash and date.
        let commit_hash = option_env!("CARGO_BUILD_COMMIT_HASH")?;
        let commit_date = option_env!("CARGO_BUILD_COMMIT_DATE")?;
        let short_commit_hash = option_env!("CARGO_BUILD_COMMIT_SHORT_HASH").unwrap_or_else(|| {
            // Fallback to first 7 characters of full hash
            #[expect(clippy::string_slice, reason = "Length is checked")]
            {
                &commit_hash[0..7.min(commit_hash.len())]
            }
        });

        let tag = option_env!("CARGO_BUILD_TAG");
        let tag_distance =
            option_env!("CARGO_BUILD_TAG_DISTANCE").and_then(|s| s.parse::<u32>().ok());

        Some(Self {
            short_commit_hash: short_commit_hash.to_owned(),
            commit_hash: commit_hash.to_owned(),
            commit_date: commit_date.to_owned(),
            tag: tag.map(str::to_owned),
            tag_distance,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;

    #[test]
    fn version_formatting_without_commit_info() {
        let version = Version {
            version: (1, 2, 3),
            commit_info: None,
        };
        assert_eq!(version.to_string(), "1.2.3");
    }

    #[test]
    fn version_formatting_at_tag() {
        let version = Version {
            version: (1, 2, 3),
            commit_info: Some(CommitInfo {
                short_commit_hash: "abcdef0".to_owned(),
                commit_hash: "abcdef0123456789abcdef0123456789abcdef01".to_owned(),
                commit_date: "2025-01-15".to_owned(),
                tag: Some("v1.2.3".to_owned()),
                tag_distance: Some(0),
            }),
        };
        assert_eq!(version.to_string(), "1.2.3 (abcdef0 2025-01-15)");
    }

    #[test]
    fn version_formatting_with_dev_commits() {
        let version = Version {
            version: (1, 2, 3),
            commit_info: Some(CommitInfo {
                short_commit_hash: "abcdef0".to_owned(),
                commit_hash: "abcdef0123456789abcdef0123456789abcdef01".to_owned(),
                commit_date: "2025-01-15".to_owned(),
                tag: Some("v1.2.3".to_owned()),
                tag_distance: Some(5),
            }),
        };
        assert_eq!(version.to_string(), "1.2.3.dev5 (abcdef0 2025-01-15)");
    }
}
