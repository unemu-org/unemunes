//! Boundary tripwires for `nes` (decision-1, decision-41 (unemupc)): the crate
//! depends on `x65` alone and its production sources use no host interface.

use std::path::Path;
use std::process::Command;

#[test]
fn nes_depends_on_x65_only() {
    let output = Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "tree",
            "--offline",
            "--locked",
            "--package",
            "nes",
            "--all-features",
            "--target",
            "all",
            "--edges",
            "normal,build",
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
    let names: Vec<&str> = tree
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.split_whitespace().next().unwrap_or(""))
        .collect();
    assert_eq!(names, ["nes", "x65"], "nes may depend on x65 only:\n{tree}");
}

/// Every path into `std` must be one of the allowed ones, so grouped imports
/// such as `use std::{fs, env}` cannot slip past; the standard streams'
/// macros and `unsafe` are refused by name.
#[test]
fn sources_use_no_host_interface() {
    const ALLOWED_STD: [&str; 2] = ["std::fmt", "std::error::Error"];
    const FORBIDDEN: [&str; 6] = [
        "println!",
        "eprintln!",
        "print!",
        "eprint!",
        "dbg!",
        "unsafe ",
    ];
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![src];
    while let Some(path) = files.pop() {
        if path.is_dir() {
            for entry in std::fs::read_dir(&path).expect("read src") {
                files.push(entry.expect("entry").path());
            }
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read source");
        for (at, _) in text.match_indices("std::") {
            let rest = &text[at..];
            assert!(
                ALLOWED_STD.iter().any(|allowed| rest.starts_with(allowed)),
                "{} uses {}",
                path.display(),
                rest.lines().next().unwrap_or(rest)
            );
        }
        for word in FORBIDDEN {
            assert!(!text.contains(word), "{} uses {word}", path.display());
        }
    }
}
