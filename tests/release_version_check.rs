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

/// Write a minimal tree with the two version declarations the script reads.
/// The Cargo fixture deliberately carries a `[package]` block above
/// `[workspace.package]`, because that is the shape of the real file and the
/// naive `grep '^version'` reads the wrong one.
fn tree(cargo_version: &str, docker_version: &str) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        format!(
            "[package]\n\
             name = \"zorp\"\n\
             version.workspace = true\n\
             \n\
             [workspace.package]\n\
             edition = \"2021\"\n\
             license = \"MIT\"\n\
             version = \"{cargo_version}\"\n"
        ),
    )
    .unwrap();
    fs::write(
        dir.path().join("Dockerfile"),
        format!("FROM debian:12-slim\nARG VERSION={docker_version}\nARG TARGETARCH\n"),
    )
    .unwrap();
    dir
}

fn check(tag: &str, cargo_version: &str, docker_version: &str) -> std::process::Output {
    let dir = tree(cargo_version, docker_version);
    Command::new("sh")
        .arg(script())
        .arg(tag)
        .arg(dir.path())
        .output()
        .unwrap()
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
