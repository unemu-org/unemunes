//! Boundary tripwires for `unemu-nes-core` (decision-41 (unemupc), conformance
//! item 14): it links only the contract and this workspace's core crates, and
//! its sources use no host interface.

use std::path::Path;
use std::process::Command;

#[test]
fn links_only_the_contract_and_the_core_crates() {
    let output = Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "tree",
            "--offline",
            "--locked",
            "--package",
            "unemu-nes-core",
            "--edges",
            "normal,build",
            "--prefix",
            "none",
            "--color",
            "never",
            "--no-dedupe",
        ])
        .output()
        .expect("run cargo tree");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8(output.stdout).expect("UTF-8");
    let mut names: Vec<&str> = tree
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(
        names,
        ["nes", "unemu-core-api", "unemu-nes-core", "x65"],
        "{tree}"
    );
}

#[test]
fn sources_use_no_host_interface() {
    const ALLOWED_STD: [&str; 5] = [
        "std::collections",
        "std::io::{Read, Write}",
        "std::mem",
        "std::rc::Rc",
        "std::str",
    ];
    const FORBIDDEN: [&str; 6] = [
        "println!",
        "eprintln!",
        "print!",
        "eprint!",
        "dbg!",
        "unsafe ",
    ];
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(&src).expect("read src") {
        let path = entry.expect("entry").path();
        let file = path.display().to_string();
        let text = std::fs::read_to_string(&path).expect("read source");
        for (at, _) in text.match_indices("std::") {
            let rest = &text[at..];
            assert!(
                ALLOWED_STD.iter().any(|allowed| rest.starts_with(allowed)),
                "{file} uses {}",
                rest.lines().next().unwrap_or(rest)
            );
        }
        for word in FORBIDDEN {
            assert!(!text.contains(word), "{file} uses {word}");
        }
    }
}
