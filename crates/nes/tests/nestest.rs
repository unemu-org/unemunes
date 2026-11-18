//! nestest (Kevin Horton, 2004) in its automation mode: the CPU starts at
//! `$C000` after the reset sequence, and every instruction of the official
//! part is compared with the reference log: PC, A, X, Y, P, S, the PPU's
//! scanline and dot, and the CPU cycle count. The part ends at the first
//! line whose mnemonic carries a `*`, the first undocumented opcode; there
//! the CPU must answer `Event::Unsupported` (decision-4) and `$02`, the
//! official tests' error code, must be 0.
//!
//! No licence was found for nestest, so it is fetched, never committed
//! (decision-3): `scripts/fetch-oracles.sh fetch nestest`. Without it the
//! test passes with a message, unless `NES_ORACLES_REQUIRED` is set, as CI
//! sets it.

use std::path::PathBuf;

use nes::{Cartridge, Config, Nes};
use x65::Event;

#[derive(Debug, PartialEq, Eq)]
struct Line {
    pc: u16,
    a: u8,
    x: u8,
    y: u8,
    p: u8,
    s: u8,
    scanline: u16,
    dot: u16,
    cycles: u64,
}

fn field<'a>(line: &'a str, key: &str) -> &'a str {
    let start = line.find(key).unwrap_or_else(|| panic!("{key} in {line}")) + key.len();
    line[start..].split_whitespace().next().expect("value")
}

fn parse(line: &str) -> Line {
    let hex8 = |key| u8::from_str_radix(field(line, key), 16).expect("hex byte");
    let ppu_at = line.find("PPU:").expect("PPU column") + 4;
    let mut ppu = line[ppu_at..].split(',');
    let scanline = ppu
        .next()
        .expect("scanline")
        .trim()
        .parse()
        .expect("scanline");
    let dot = ppu
        .next()
        .expect("dot")
        .split_whitespace()
        .next()
        .expect("dot")
        .parse()
        .expect("dot");
    Line {
        pc: u16::from_str_radix(&line[0..4], 16).expect("PC"),
        a: hex8("A:"),
        x: hex8("X:"),
        y: hex8("Y:"),
        p: hex8("P:"),
        s: hex8("SP:"),
        scanline,
        dot,
        cycles: field(line, "CYC:").parse().expect("CYC"),
    }
}

/// The mnemonic column: an undocumented instruction is marked with `*`.
fn undocumented(line: &str) -> bool {
    line.as_bytes().get(15) == Some(&b'*')
}

fn oracles() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.oracles/nestest");
    dir.join("nestest.log").is_file().then_some(dir)
}

#[test]
fn official_part_matches_the_log() {
    let Some(dir) = oracles() else {
        assert!(
            std::env::var_os("NES_ORACLES_REQUIRED").is_none(),
            "NES_ORACLES_REQUIRED is set and nestest is missing"
        );
        eprintln!("skipped: nestest not found; run scripts/fetch-oracles.sh fetch nestest");
        return;
    };
    let image = std::fs::read(dir.join("nestest.nes")).expect("nestest.nes");
    let log = std::fs::read_to_string(dir.join("nestest.log")).expect("nestest.log");

    let cartridge = Cartridge::from_ines(&image).expect("nestest is an iNES image");
    let mut nes = Nes::new(&cartridge, Config::default()).expect("NROM");
    assert_eq!(nes.step(), Event::Reset);
    let mut cpu = nes.cpu();
    cpu.pc = 0xC000;
    nes.set_cpu(cpu);

    let mut compared = 0;
    for (number, text) in log.lines().enumerate() {
        let want = parse(text);
        let cpu = nes.cpu();
        let (scanline, dot) = nes.ppu().position();
        let got = Line {
            pc: cpu.pc,
            a: cpu.a,
            x: cpu.x,
            y: cpu.y,
            p: cpu.p,
            s: cpu.s,
            scanline,
            dot,
            cycles: nes.cpu_cycles(),
        };
        assert_eq!(got, want, "line {}: {text}", number + 1);
        if undocumented(text) {
            let opcode = u8::from_str_radix(&text[6..8], 16).expect("opcode");
            assert_eq!(nes.step(), Event::Unsupported { opcode });
            break;
        }
        assert!(
            matches!(nes.step(), Event::Instruction { .. }),
            "line {}",
            number + 1
        );
        compared += 1;
    }
    assert_eq!(compared, 5003, "the official part is 5003 instructions");
    assert_eq!(nes.ram()[0x02], 0, "official tests' error code at $02");
}
