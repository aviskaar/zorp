//! The Release workflow refuses to publish a tag whose version disagrees
//! with the workspace manifest or the Dockerfile's `ARG VERSION` default.
//!
//! That check runs only on a tag push, so nothing exercises it until the
//! moment it has to work, on the one run where being wrong is expensive.
//! These tests run the same script the workflow runs, against fixtures, so
//! its logic is covered on every pull request.
//!
//! The drift it exists to catch is not hypothetical: v0.3.0 and v0.3.1 both
//! shipped a binary reporting 0.2.1, and a bare `docker build` fetched the
//! v0.2.1 release on purpose, because both defaults had been left behind.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/check-release-version.sh")
}

/// Write a minimal tree with the version declarations the script reads.
/// The Cargo fixture deliberately carries a `[package]` block above
/// `[workspace.package]`, because that is the shape of the real file and the
/// naive `grep '^version'` reads the wrong one.
///
/// `members` is a list of `(path, inherits)` pairs under the workspace root.
/// `inherits == true` writes `version.workspace = true`; false pins `0.1.0`.
fn tree(
    cargo_version: &str,
    docker_version: &str,
    members: &[(&str, bool)],
) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let member_list = members
        .iter()
        .map(|(path, _)| format!("\"{path}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        dir.path().join("Cargo.toml"),
        format!(
            "[package]\n\
             name = \"zorp\"\n\
             version.workspace = true\n\
             \n\
             [workspace]\n\
             members = [{member_list}]\n\
             \n\
             [workspace.package]\n\
             edition = \"2021\"\n\
             license = \"MIT\"\n\
             version = \"{cargo_version}\"\n"
        ),
    )
    .unwrap();
    for (path, inherits) in members {
        if *path == "." {
            continue;
        }
        let member_dir = dir.path().join(path);
        fs::create_dir_all(&member_dir).unwrap();
        let version_line = if *inherits {
            "version.workspace = true\n".to_string()
        } else {
            "version = \"0.1.0\"\n".to_string()
        };
        fs::write(
            member_dir.join("Cargo.toml"),
            format!(
                "[package]\n\
                 name = \"{path}\"\n\
                 {version_line}\
                 edition.workspace = true\n"
            ),
        )
        .unwrap();
    }
    fs::write(
        dir.path().join("Dockerfile"),
        format!("FROM debian:12-slim\nARG VERSION={docker_version}\nARG TARGETARCH\n"),
    )
    .unwrap();
    dir
}

fn check_with_members(
    tag: &str,
    cargo_version: &str,
    docker_version: &str,
    members: &[(&str, bool)],
) -> std::process::Output {
    let dir = tree(cargo_version, docker_version, members);
    Command::new("sh")
        .arg(script())
        .arg(tag)
        .arg(dir.path())
        .output()
        .unwrap()
}

fn check(tag: &str, cargo_version: &str, docker_version: &str) -> std::process::Output {
    check_with_members(
        tag,
        cargo_version,
        docker_version,
        &[(".", true), ("zorp-agent", true)],
    )
}

#[test]
fn agreeing_versions_pass() {
    let out = check("v0.3.1", "0.3.1", "v0.3.1");
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_stale_manifest_fails_the_release() {
    // Exactly what shipped: tag moved, workspace version did not.
    let out = check("v0.3.1", "0.2.1", "v0.3.1");
    assert!(!out.status.success(), "a stale manifest must fail the tag");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(all.contains("Cargo.toml"), "must name the file: {all}");
}

#[test]
fn a_stale_dockerfile_default_fails_the_release() {
    let out = check("v0.3.1", "0.3.1", "v0.2.1");
    assert!(
        !out.status.success(),
        "a stale ARG VERSION must fail the tag"
    );
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(all.contains("Dockerfile"), "must name the file: {all}");
}

/// Both wrong must report both, not stop at the first. A release blocked
/// twice in a row over one problem at a time is how people learn to reach
/// for --no-verify.
#[test]
fn both_stale_reports_both() {
    let out = check("v0.4.0", "0.3.1", "v0.3.1");
    assert!(!out.status.success());
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(all.contains("Cargo.toml"), "missing Cargo.toml: {all}");
    assert!(all.contains("Dockerfile"), "missing Dockerfile: {all}");
}

/// The script compares a `v`-prefixed tag against a bare Cargo version. A
/// tag that merely contains the version is not a match.
#[test]
fn a_prefix_match_is_not_a_match() {
    let out = check("v0.3.10", "0.3.1", "v0.3.10");
    assert!(
        !out.status.success(),
        "v0.3.10 must not be accepted against version 0.3.1"
    );
}

/// A product member that pins its own version instead of inheriting must
/// fail — this is the zorp-search shape that slipped past the old gate.
#[test]
fn a_member_with_its_own_version_fails() {
    let out = check_with_members(
        "v0.3.2",
        "0.3.2",
        "v0.3.2",
        &[(".", true), ("zorp-search", false)],
    );
    assert!(
        !out.status.success(),
        "pinned member version must fail the tag"
    );
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        all.contains("zorp-search"),
        "must name the member: {all}"
    );
}

/// erbga is the deliberate exemption: standalone prior work that keeps its
/// own version. It must not fail the release.
#[test]
fn erbga_may_pin_its_own_version() {
    let out = check_with_members(
        "v0.3.2",
        "0.3.2",
        "v0.3.2",
        &[(".", true), ("erbga", false), ("zorp-agent", true)],
    );
    assert!(
        out.status.success(),
        "erbga exemption must pass: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}