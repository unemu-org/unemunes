//! Boundary tripwires for `x65` (decision-1): the crate depends on nothing,
//! uses no `std` and forbids unsafe code, so it can leave this workspace as a
//! move of one directory.

use std::path::Path;
use std::process::Command;

/// Cargo resolves every dependency form, optional and target-specific
/// normal, build and development dependencies included, so no TOML parser is
/// needed here.
#[test]
fn x65_has_no_dependencies() {
    let output = Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "tree",
            "--offline",
            "--locked",
            "--package",
            "x65",
            "--all-features",
            "--target",
            "all",
            "--edges",
            "normal,build,dev",
            "--depth",
            "1",
            "--prefix",
            "none",
            "--color",
            "never",
        ])
        .output()
        .expect("run cargo tree");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8(output.stdout).expect("cargo tree prints UTF-8");
    let mut lines = tree.lines().filter(|line| !line.trim().is_empty());
    assert!(
        lines.next().is_some_and(|line| line.starts_with("x65 v")),
        "expected x65 as the root of its dependency tree:\n{tree}"
    );
    assert!(
        lines.next().is_none(),
        "x65 must have no dependency of any kind (decision-1); put oracle \
         harnesses that need one in a crate of their own. Found:\n{tree}"
    );
}

#[test]
fn x65_root_declares_no_std_and_forbids_unsafe() {
    let lib = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let source = std::fs::read_to_string(&lib).expect("read src/lib.rs");
    for attribute in ["#![no_std]", "#![forbid(unsafe_code)]"] {
        assert!(
            source.lines().any(|line| line.trim() == attribute),
            "{} must declare {attribute} (decision-1)",
            lib.display()
        );
    }
}
