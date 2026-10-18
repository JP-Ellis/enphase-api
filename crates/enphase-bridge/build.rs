//! Cargo build script.
//!
//! The build script is used by Cargo to perform additional tasks during the
//! build process. Cargo will execute the [`main`] function.

#![expect(clippy::expect_used, reason = "Build script should panic on error")]
#![expect(
    clippy::print_stderr,
    reason = "Build script should print errors to stderr"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The prefix used for version tags in Git.
const VERSION_PREFIX: &str = "enphase-bridge/";

fn main() {
    if let Some(repo) = locate_git_dir() {
        rerun_on_git_changes(&repo);
        expose_commit_info(&repo)
            .inspect_err(|err| {
                eprintln!("Failed to expose commit info: {err}");
            })
            .expect("Failed to expose commit info");
    }
}

/// Locate the Git repository.
///
/// This function attempts to find the Git repository by traversing
/// upwards from the current directory. If a `.git` directory is found,
/// it returns the path to the repository. If no Git repository is found,
/// it returns `None`.
fn locate_git_dir() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().expect("Failed to get current directory");
    loop {
        if dir.join(".git").is_dir() {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Rerun the build script if the Git repository has changed.
///
/// This function will add `rerun-if-changed` directives for the Git
/// repository's HEAD file and index. If either changes, the build script will
/// be re-executed.
fn rerun_on_git_changes(path: &Path) {
    println!("cargo:rerun-if-changed={}/HEAD", path.display());
    println!("cargo:rerun-if-changed={}/index", path.display());
}

/// Expose commit information from Git.
///
/// This function will attempt to retrieve and expose the following information:
///
/// - `CARGO_BUILD_VERSION`: The version string derived from the latest tag, or
///   taken from Cargo.toml if no tag is found.
/// - `CARGO_BUILD_VERSION_DISTANCE`: The number of commits since the last tag.
///   This will be `0` if the current commit is exactly at a tag.
/// - `CARGO_BUILD_COMMIT_HASH`: The full commit hash.
/// - `CARGO_BUILD_COMMIT_SHORT_HASH`: The short commit hash.
/// - `CARGO_BUILD_COMMIT_DATE`: The commit date in YYYY-MM-DD format.
///
/// If the Git repository is not available or information cannot be retrieved,
/// only `CARGO_BUILD_VERSION` will be set.
fn expose_commit_info(path: &Path) -> Result<(), Box<dyn core::error::Error>> {
    let output = Command::new("git")
        .arg("log")
        .arg("-1")
        .arg("--date=short")
        .arg(format!(
            "--format=%H %h %cd %(describe:tags,match={VERSION_PREFIX}*)"
        ))
        .current_dir(path)
        .output()?
        .stdout;

    let stdout = String::from_utf8(output)?;
    let mut parts = stdout.split_whitespace();

    // Hash is always available
    println!(
        "cargo:rustc-env=CARGO_BUILD_COMMIT_HASH={}",
        parts.next().expect("Expected commit hash")
    );
    println!(
        "cargo:rustc-env=CARGO_BUILD_COMMIT_SHORT_HASH={}",
        parts.next().expect("Expected short commit hash")
    );
    println!(
        "cargo:rustc-env=CARGO_BUILD_COMMIT_DATE={}",
        parts.next().expect("Expected commit date")
    );

    // The describe part may not always be available
    if let Some(describe) = parts.next() {
        let mut sub_parts = describe.rsplitn(3, '-');
        sub_parts.next(); // Skip the commit hash part
        println!(
            "cargo:rustc-env=CARGO_BUILD_TAG_DISTANCE={}",
            sub_parts.next().expect("Expected tag distance")
        );
        println!(
            "cargo:rustc-env=CARGO_BUILD_TAG={}",
            sub_parts.next().expect("Expected the version")
        );
    }

    Ok(())
}
