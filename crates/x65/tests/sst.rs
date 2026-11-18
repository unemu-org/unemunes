//! SingleStepTests 65x02, `nes6502/v1`: one file per opcode, 10 000 tests
//! each, every test an initial state, a final state and the bus activity of
//! each cycle. MIT License, "Copyright (c) 2024 Thomas Harte et al"; fetched
//! by `scripts/fetch-oracles.sh fetch sst-nes6502` (decision-3) and pinned in
//! `scripts/oracles.manifest`. The running witness (decision-2) for the
//! cycle sequences of the MCS6500 Hardware Manual (MOS Technology, January
//! 1976), Appendix A, and the operations of the Programming Manual.
//!
//! A documented opcode must reproduce the final registers, the final memory
//! and every cycle's address, data and direction. An undocumented opcode must
//! answer `Event::Unsupported` after one fetch cycle with nothing changed
//! (decision-4), until milestone N1 implements it.
//!
//! The corpus lives in `.oracles/` at the workspace root, or in the directory
//! named by `X65_SST_DIR`. Without it every test passes with a message that
//! names the fetch command, unless `X65_SST_REQUIRED` is set, as CI sets it.

use std::path::PathBuf;

use x65::{Bus, Cpu, Event, Lines, Model};

/// The 151 documented opcodes of the NMOS 6502: MCS6500 Programming Manual,
/// Appendix D (operation codes in hexadecimal sequence). Written out here,
/// not taken from the implementation, so the witness stays independent.
const DOCUMENTED: [u8; 151] = [
    0x00, 0x01, 0x05, 0x06, 0x08, 0x09, 0x0A, 0x0D, 0x0E, 0x10, 0x11, 0x15, 0x16, 0x18, 0x19, 0x1D,
    0x1E, 0x20, 0x21, 0x24, 0x25, 0x26, 0x28, 0x29, 0x2A, 0x2C, 0x2D, 0x2E, 0x30, 0x31, 0x35, 0x36,
    0x38, 0x39, 0x3D, 0x3E, 0x40, 0x41, 0x45, 0x46, 0x48, 0x49, 0x4A, 0x4C, 0x4D, 0x4E, 0x50, 0x51,
    0x55, 0x56, 0x58, 0x59, 0x5D, 0x5E, 0x60, 0x61, 0x65, 0x66, 0x68, 0x69, 0x6A, 0x6C, 0x6D, 0x6E,
    0x70, 0x71, 0x75, 0x76, 0x78, 0x79, 0x7D, 0x7E, 0x81, 0x84, 0x85, 0x86, 0x88, 0x8A, 0x8C, 0x8D,
    0x8E, 0x90, 0x91, 0x94, 0x95, 0x96, 0x98, 0x99, 0x9A, 0x9D, 0xA0, 0xA1, 0xA2, 0xA4, 0xA5, 0xA6,
    0xA8, 0xA9, 0xAA, 0xAC, 0xAD, 0xAE, 0xB0, 0xB1, 0xB4, 0xB5, 0xB6, 0xB8, 0xB9, 0xBA, 0xBC, 0xBD,
    0xBE, 0xC0, 0xC1, 0xC4, 0xC5, 0xC6, 0xC8, 0xC9, 0xCA, 0xCC, 0xCD, 0xCE, 0xD0, 0xD1, 0xD5, 0xD6,
    0xD8, 0xD9, 0xDD, 0xDE, 0xE0, 0xE1, 0xE4, 0xE5, 0xE6, 0xE8, 0xE9, 0xEA, 0xEC, 0xED, 0xEE, 0xF0,
    0xF1, 0xF5, 0xF6, 0xF8, 0xF9, 0xFD, 0xFE,
];

#[test]
fn documented_list_has_151_distinct_opcodes() {
    let mut seen = [false; 256];
    for opcode in DOCUMENTED {
        assert!(!seen[usize::from(opcode)], "duplicate {opcode:02X}");
        seen[usize::from(opcode)] = true;
    }
}

// ----- a minimal JSON reader for the corpus' shape -----

#[derive(Debug)]
enum Json {
    Number(i64),
    Text(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> &Json {
        match self {
            Json::Object(fields) => fields
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value)
                .unwrap_or_else(|| panic!("missing field {key}")),
            _ => panic!("not an object looking for {key}"),
        }
    }
    fn number(&self) -> i64 {
        match self {
            Json::Number(n) => *n,
            other => panic!("not a number: {other:?}"),
        }
    }
    fn text(&self) -> &str {
        match self {
            Json::Text(t) => t,
            other => panic!("not text: {other:?}"),
        }
    }
    fn array(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            other => panic!("not an array: {other:?}"),
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn skip_space(&mut self) {
        while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }
    fn expect(&mut self, byte: u8) {
        self.skip_space();
        assert_eq!(self.bytes[self.at], byte, "at byte {}", self.at);
        self.at += 1;
    }
    fn peek(&mut self) -> u8 {
        self.skip_space();
        self.bytes[self.at]
    }
    fn value(&mut self) -> Json {
        match self.peek() {
            b'{' => {
                self.at += 1;
                let mut fields = Vec::new();
                if self.peek() == b'}' {
                    self.at += 1;
                    return Json::Object(fields);
                }
                loop {
                    let key = match self.value() {
                        Json::Text(key) => key,
                        other => panic!("object key {other:?}"),
                    };
                    self.expect(b':');
                    fields.push((key, self.value()));
                    match self.peek() {
                        b',' => self.at += 1,
                        b'}' => {
                            self.at += 1;
                            return Json::Object(fields);
                        }
                        other => panic!("unexpected {} in object", other as char),
                    }
                }
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                if self.peek() == b']' {
                    self.at += 1;
                    return Json::Array(items);
                }
                loop {
                    items.push(self.value());
                    match self.peek() {
                        b',' => self.at += 1,
                        b']' => {
                            self.at += 1;
                            return Json::Array(items);
                        }
                        other => panic!("unexpected {} in array", other as char),
                    }
                }
            }
            b'"' => {
                self.at += 1;
                let start = self.at;
                while self.bytes[self.at] != b'"' {
                    assert_ne!(self.bytes[self.at], b'\\', "escapes are not expected");
                    self.at += 1;
                }
                let text = String::from_utf8(self.bytes[start..self.at].to_vec()).expect("UTF-8");
                self.at += 1;
                Json::Text(text)
            }
            _ => {
                let start = self.at;
                while self.at < self.bytes.len()
                    && (self.bytes[self.at] == b'-' || self.bytes[self.at].is_ascii_digit())
                {
                    self.at += 1;
                }
                let digits = std::str::from_utf8(&self.bytes[start..self.at]).expect("ASCII");
                Json::Number(
                    digits
                        .parse()
                        .unwrap_or_else(|_| panic!("number at {start}")),
                )
            }
        }
    }
}

// ----- the bus of a test: sparse memory and a log of cycles -----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cycle {
    address: u16,
    value: u8,
    write: bool,
}

struct TestBus {
    memory: Vec<(u16, u8)>,
    cycles: Vec<Cycle>,
}

impl TestBus {
    fn cell(&mut self, address: u16) -> &mut u8 {
        if let Some(index) = self.memory.iter().position(|(a, _)| *a == address) {
            &mut self.memory[index].1
        } else {
            self.memory.push((address, 0));
            &mut self.memory.last_mut().expect("just pushed").1
        }
    }
}

impl Bus for TestBus {
    fn read(&mut self, address: u16) -> u8 {
        let value = *self.cell(address);
        self.cycles.push(Cycle {
            address,
            value,
            write: false,
        });
        value
    }
    fn write(&mut self, address: u16, value: u8) {
        *self.cell(address) = value;
        self.cycles.push(Cycle {
            address,
            value,
            write: true,
        });
    }
    fn lines(&mut self) -> Lines {
        Lines::default()
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Registers {
    pc: u16,
    s: u8,
    a: u8,
    x: u8,
    y: u8,
    p: u8,
}

fn registers(state: &Json) -> Registers {
    Registers {
        pc: state.get("pc").number() as u16,
        s: state.get("s").number() as u8,
        a: state.get("a").number() as u8,
        x: state.get("x").number() as u8,
        y: state.get("y").number() as u8,
        p: state.get("p").number() as u8,
    }
}

fn memory(state: &Json) -> Vec<(u16, u8)> {
    state
        .get("ram")
        .array()
        .iter()
        .map(|cell| {
            let pair = cell.array();
            (pair[0].number() as u16, pair[1].number() as u8)
        })
        .collect()
}

fn corpus() -> Option<PathBuf> {
    let dir = match std::env::var_os("X65_SST_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.oracles/sst-nes6502/v1"),
    };
    dir.is_dir().then_some(dir)
}

/// Run one test; describe the first difference, if any.
fn check(opcode: u8, documented: bool, test: &Json) -> Option<String> {
    let initial = test.get("initial");
    let before = registers(initial);
    let mut bus = TestBus {
        memory: memory(initial),
        cycles: Vec::new(),
    };

    let mut cpu = Cpu::new(Model::Rp2a03);
    let mut state = cpu.state();
    state.pc = before.pc;
    state.s = before.s;
    state.a = before.a;
    state.x = before.x;
    state.y = before.y;
    state.p = before.p;
    state.cycle = 0;
    state.reset_pending = false;
    cpu.set_state(state);

    let event = cpu.step(&mut bus);
    let after = cpu.state();
    let got = Registers {
        pc: after.pc,
        s: after.s,
        a: after.a,
        x: after.x,
        y: after.y,
        p: after.p,
    };

    if !documented {
        let fetch = Cycle {
            address: before.pc,
            value: opcode,
            write: false,
        };
        if event != (Event::Unsupported { opcode }) || got != before || bus.cycles != [fetch] {
            return Some(format!(
                "undocumented: event {event:?}, registers {got:?}, cycles {:?}",
                bus.cycles
            ));
        }
        return None;
    }

    if event != (Event::Instruction { opcode }) {
        return Some(format!("event {event:?}"));
    }
    let want = registers(test.get("final"));
    if got != want {
        return Some(format!("registers {got:?}, want {want:?}"));
    }
    for (address, value) in memory(test.get("final")) {
        let have = *bus.cell(address);
        if have != value {
            return Some(format!(
                "memory {address:04X} = {have:02X}, want {value:02X}"
            ));
        }
    }
    let want: Vec<Cycle> = test
        .get("cycles")
        .array()
        .iter()
        .map(|cycle| {
            let parts = cycle.array();
            Cycle {
                address: parts[0].number() as u16,
                value: parts[1].number() as u8,
                write: parts[2].text() == "write",
            }
        })
        .collect();
    if bus.cycles != want {
        return Some(format!("cycles {:?}, want {want:?}", bus.cycles));
    }
    None
}

fn run(opcode: u8) {
    let Some(dir) = corpus() else {
        assert!(
            std::env::var_os("X65_SST_REQUIRED").is_none(),
            "X65_SST_REQUIRED is set and the corpus is missing"
        );
        eprintln!(
            "skipped: SingleStepTests nes6502/v1 not found; run \
             scripts/fetch-oracles.sh fetch sst-nes6502"
        );
        return;
    };
    let path = dir.join(format!("{opcode:02x}.json"));
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let tests = Reader {
        bytes: &bytes,
        at: 0,
    }
    .value();
    let documented = DOCUMENTED.contains(&opcode);
    let mut failures = 0;
    let mut first = Vec::new();
    for test in tests.array() {
        if let Some(difference) = check(opcode, documented, test) {
            failures += 1;
            if first.len() < 3 {
                first.push(format!("{}: {difference}", test.get("name").text()));
            }
        }
    }
    assert!(
        failures == 0,
        "opcode {opcode:02X}: {failures} of {} tests differ; first:\n{}",
        tests.array().len(),
        first.join("\n")
    );
}

#[test]
fn opcode_00() {
    run(0x00);
}
#[test]
fn opcode_01() {
    run(0x01);
}
#[test]
fn opcode_02() {
    run(0x02);
}
#[test]
fn opcode_03() {
    run(0x03);
}
#[test]
fn opcode_04() {
    run(0x04);
}
#[test]
fn opcode_05() {
    run(0x05);
}
#[test]
fn opcode_06() {
    run(0x06);
}
#[test]
fn opcode_07() {
    run(0x07);
}
#[test]
fn opcode_08() {
    run(0x08);
}
#[test]
fn opcode_09() {
    run(0x09);
}
#[test]
fn opcode_0a() {
    run(0x0A);
}
#[test]
fn opcode_0b() {
    run(0x0B);
}
#[test]
fn opcode_0c() {
    run(0x0C);
}
#[test]
fn opcode_0d() {
    run(0x0D);
}
#[test]
fn opcode_0e() {
    run(0x0E);
}
#[test]
fn opcode_0f() {
    run(0x0F);
}
#[test]
fn opcode_10() {
    run(0x10);
}
#[test]
fn opcode_11() {
    run(0x11);
}
#[test]
fn opcode_12() {
    run(0x12);
}
#[test]
fn opcode_13() {
    run(0x13);
}
#[test]
fn opcode_14() {
    run(0x14);
}
#[test]
fn opcode_15() {
    run(0x15);
}
#[test]
fn opcode_16() {
    run(0x16);
}
#[test]
fn opcode_17() {
    run(0x17);
}
#[test]
fn opcode_18() {
    run(0x18);
}
#[test]
fn opcode_19() {
    run(0x19);
}
#[test]
fn opcode_1a() {
    run(0x1A);
}
#[test]
fn opcode_1b() {
    run(0x1B);
}
#[test]
fn opcode_1c() {
    run(0x1C);
}
#[test]
fn opcode_1d() {
    run(0x1D);
}
#[test]
fn opcode_1e() {
    run(0x1E);
}
#[test]
fn opcode_1f() {
    run(0x1F);
}
#[test]
fn opcode_20() {
    run(0x20);
}
#[test]
fn opcode_21() {
    run(0x21);
}
#[test]
fn opcode_22() {
    run(0x22);
}
#[test]
fn opcode_23() {
    run(0x23);
}
#[test]
fn opcode_24() {
    run(0x24);
}
#[test]
fn opcode_25() {
    run(0x25);
}
#[test]
fn opcode_26() {
    run(0x26);
}
#[test]
fn opcode_27() {
    run(0x27);
}
#[test]
fn opcode_28() {
    run(0x28);
}
#[test]
fn opcode_29() {
    run(0x29);
}
#[test]
fn opcode_2a() {
    run(0x2A);
}
#[test]
fn opcode_2b() {
    run(0x2B);
}
#[test]
fn opcode_2c() {
    run(0x2C);
}
#[test]
fn opcode_2d() {
    run(0x2D);
}
#[test]
fn opcode_2e() {
    run(0x2E);
}
#[test]
fn opcode_2f() {
    run(0x2F);
}
#[test]
fn opcode_30() {
    run(0x30);
}
#[test]
fn opcode_31() {
    run(0x31);
}
#[test]
fn opcode_32() {
    run(0x32);
}
#[test]
fn opcode_33() {
    run(0x33);
}
#[test]
fn opcode_34() {
    run(0x34);
}
#[test]
fn opcode_35() {
    run(0x35);
}
#[test]
fn opcode_36() {
    run(0x36);
}
#[test]
fn opcode_37() {
    run(0x37);
}
#[test]
fn opcode_38() {
    run(0x38);
}
#[test]
fn opcode_39() {
    run(0x39);
}
#[test]
fn opcode_3a() {
    run(0x3A);
}
#[test]
fn opcode_3b() {
    run(0x3B);
}
#[test]
fn opcode_3c() {
    run(0x3C);
}
#[test]
fn opcode_3d() {
    run(0x3D);
}
#[test]
fn opcode_3e() {
    run(0x3E);
}
#[test]
fn opcode_3f() {
    run(0x3F);
}
#[test]
fn opcode_40() {
    run(0x40);
}
#[test]
fn opcode_41() {
    run(0x41);
}
#[test]
fn opcode_42() {
    run(0x42);
}
#[test]
fn opcode_43() {
    run(0x43);
}
#[test]
fn opcode_44() {
    run(0x44);
}
#[test]
fn opcode_45() {
    run(0x45);
}
#[test]
fn opcode_46() {
    run(0x46);
}
#[test]
fn opcode_47() {
    run(0x47);
}
#[test]
fn opcode_48() {
    run(0x48);
}
#[test]
fn opcode_49() {
    run(0x49);
}
#[test]
fn opcode_4a() {
    run(0x4A);
}
#[test]
fn opcode_4b() {
    run(0x4B);
}
#[test]
fn opcode_4c() {
    run(0x4C);
}
#[test]
fn opcode_4d() {
    run(0x4D);
}
#[test]
fn opcode_4e() {
    run(0x4E);
}
#[test]
fn opcode_4f() {
    run(0x4F);
}
#[test]
fn opcode_50() {
    run(0x50);
}
#[test]
fn opcode_51() {
    run(0x51);
}
#[test]
fn opcode_52() {
    run(0x52);
}
#[test]
fn opcode_53() {
    run(0x53);
}
#[test]
fn opcode_54() {
    run(0x54);
}
#[test]
fn opcode_55() {
    run(0x55);
}
#[test]
fn opcode_56() {
    run(0x56);
}
#[test]
fn opcode_57() {
    run(0x57);
}
#[test]
fn opcode_58() {
    run(0x58);
}
#[test]
fn opcode_59() {
    run(0x59);
}
#[test]
fn opcode_5a() {
    run(0x5A);
}
#[test]
fn opcode_5b() {
    run(0x5B);
}
#[test]
fn opcode_5c() {
    run(0x5C);
}
#[test]
fn opcode_5d() {
    run(0x5D);
}
#[test]
fn opcode_5e() {
    run(0x5E);
}
#[test]
fn opcode_5f() {
    run(0x5F);
}
#[test]
fn opcode_60() {
    run(0x60);
}
#[test]
fn opcode_61() {
    run(0x61);
}
#[test]
fn opcode_62() {
    run(0x62);
}
#[test]
fn opcode_63() {
    run(0x63);
}
#[test]
fn opcode_64() {
    run(0x64);
}
#[test]
fn opcode_65() {
    run(0x65);
}
#[test]
fn opcode_66() {
    run(0x66);
}
#[test]
fn opcode_67() {
    run(0x67);
}
#[test]
fn opcode_68() {
    run(0x68);
}
#[test]
fn opcode_69() {
    run(0x69);
}
#[test]
fn opcode_6a() {
    run(0x6A);
}
#[test]
fn opcode_6b() {
    run(0x6B);
}
#[test]
fn opcode_6c() {
    run(0x6C);
}
#[test]
fn opcode_6d() {
    run(0x6D);
}
#[test]
fn opcode_6e() {
    run(0x6E);
}
#[test]
fn opcode_6f() {
    run(0x6F);
}
#[test]
fn opcode_70() {
    run(0x70);
}
#[test]
fn opcode_71() {
    run(0x71);
}
#[test]
fn opcode_72() {
    run(0x72);
}
#[test]
fn opcode_73() {
    run(0x73);
}
#[test]
fn opcode_74() {
    run(0x74);
}
#[test]
fn opcode_75() {
    run(0x75);
}
#[test]
fn opcode_76() {
    run(0x76);
}
#[test]
fn opcode_77() {
    run(0x77);
}
#[test]
fn opcode_78() {
    run(0x78);
}
#[test]
fn opcode_79() {
    run(0x79);
}
#[test]
fn opcode_7a() {
    run(0x7A);
}
#[test]
fn opcode_7b() {
    run(0x7B);
}
#[test]
fn opcode_7c() {
    run(0x7C);
}
#[test]
fn opcode_7d() {
    run(0x7D);
}
#[test]
fn opcode_7e() {
    run(0x7E);
}
#[test]
fn opcode_7f() {
    run(0x7F);
}
#[test]
fn opcode_80() {
    run(0x80);
}
#[test]
fn opcode_81() {
    run(0x81);
}
#[test]
fn opcode_82() {
    run(0x82);
}
#[test]
fn opcode_83() {
    run(0x83);
}
#[test]
fn opcode_84() {
    run(0x84);
}
#[test]
fn opcode_85() {
    run(0x85);
}
#[test]
fn opcode_86() {
    run(0x86);
}
#[test]
fn opcode_87() {
    run(0x87);
}
#[test]
fn opcode_88() {
    run(0x88);
}
#[test]
fn opcode_89() {
    run(0x89);
}
#[test]
fn opcode_8a() {
    run(0x8A);
}
#[test]
fn opcode_8b() {
    run(0x8B);
}
#[test]
fn opcode_8c() {
    run(0x8C);
}
#[test]
fn opcode_8d() {
    run(0x8D);
}
#[test]
fn opcode_8e() {
    run(0x8E);
}
#[test]
fn opcode_8f() {
    run(0x8F);
}
#[test]
fn opcode_90() {
    run(0x90);
}
#[test]
fn opcode_91() {
    run(0x91);
}
#[test]
fn opcode_92() {
    run(0x92);
}
#[test]
fn opcode_93() {
    run(0x93);
}
#[test]
fn opcode_94() {
    run(0x94);
}
#[test]
fn opcode_95() {
    run(0x95);
}
#[test]
fn opcode_96() {
    run(0x96);
}
#[test]
fn opcode_97() {
    run(0x97);
}
#[test]
fn opcode_98() {
    run(0x98);
}
#[test]
fn opcode_99() {
    run(0x99);
}
#[test]
fn opcode_9a() {
    run(0x9A);
}
#[test]
fn opcode_9b() {
    run(0x9B);
}
#[test]
fn opcode_9c() {
    run(0x9C);
}
#[test]
fn opcode_9d() {
    run(0x9D);
}
#[test]
fn opcode_9e() {
    run(0x9E);
}
#[test]
fn opcode_9f() {
    run(0x9F);
}
#[test]
fn opcode_a0() {
    run(0xA0);
}
#[test]
fn opcode_a1() {
    run(0xA1);
}
#[test]
fn opcode_a2() {
    run(0xA2);
}
#[test]
fn opcode_a3() {
    run(0xA3);
}
#[test]
fn opcode_a4() {
    run(0xA4);
}
#[test]
fn opcode_a5() {
    run(0xA5);
}
#[test]
fn opcode_a6() {
    run(0xA6);
}
#[test]
fn opcode_a7() {
    run(0xA7);
}
#[test]
fn opcode_a8() {
    run(0xA8);
}
#[test]
fn opcode_a9() {
    run(0xA9);
}
#[test]
fn opcode_aa() {
    run(0xAA);
}
#[test]
fn opcode_ab() {
    run(0xAB);
}
#[test]
fn opcode_ac() {
    run(0xAC);
}
#[test]
fn opcode_ad() {
    run(0xAD);
}
#[test]
fn opcode_ae() {
    run(0xAE);
}
#[test]
fn opcode_af() {
    run(0xAF);
}
#[test]
fn opcode_b0() {
    run(0xB0);
}
#[test]
fn opcode_b1() {
    run(0xB1);
}
#[test]
fn opcode_b2() {
    run(0xB2);
}
#[test]
fn opcode_b3() {
    run(0xB3);
}
#[test]
fn opcode_b4() {
    run(0xB4);
}
#[test]
fn opcode_b5() {
    run(0xB5);
}
#[test]
fn opcode_b6() {
    run(0xB6);
}
#[test]
fn opcode_b7() {
    run(0xB7);
}
#[test]
fn opcode_b8() {
    run(0xB8);
}
#[test]
fn opcode_b9() {
    run(0xB9);
}
#[test]
fn opcode_ba() {
    run(0xBA);
}
#[test]
fn opcode_bb() {
    run(0xBB);
}
#[test]
fn opcode_bc() {
    run(0xBC);
}
#[test]
fn opcode_bd() {
    run(0xBD);
}
#[test]
fn opcode_be() {
    run(0xBE);
}
#[test]
fn opcode_bf() {
    run(0xBF);
}
#[test]
fn opcode_c0() {
    run(0xC0);
}
#[test]
fn opcode_c1() {
    run(0xC1);
}
#[test]
fn opcode_c2() {
    run(0xC2);
}
#[test]
fn opcode_c3() {
    run(0xC3);
}
#[test]
fn opcode_c4() {
    run(0xC4);
}
#[test]
fn opcode_c5() {
    run(0xC5);
}
#[test]
fn opcode_c6() {
    run(0xC6);
}
#[test]
fn opcode_c7() {
    run(0xC7);
}
#[test]
fn opcode_c8() {
    run(0xC8);
}
#[test]
fn opcode_c9() {
    run(0xC9);
}
#[test]
fn opcode_ca() {
    run(0xCA);
}
#[test]
fn opcode_cb() {
    run(0xCB);
}
#[test]
fn opcode_cc() {
    run(0xCC);
}
#[test]
fn opcode_cd() {
    run(0xCD);
}
#[test]
fn opcode_ce() {
    run(0xCE);
}
#[test]
fn opcode_cf() {
    run(0xCF);
}
#[test]
fn opcode_d0() {
    run(0xD0);
}
#[test]
fn opcode_d1() {
    run(0xD1);
}
#[test]
fn opcode_d2() {
    run(0xD2);
}
#[test]
fn opcode_d3() {
    run(0xD3);
}
#[test]
fn opcode_d4() {
    run(0xD4);
}
#[test]
fn opcode_d5() {
    run(0xD5);
}
#[test]
fn opcode_d6() {
    run(0xD6);
}
#[test]
fn opcode_d7() {
    run(0xD7);
}
#[test]
fn opcode_d8() {
    run(0xD8);
}
#[test]
fn opcode_d9() {
    run(0xD9);
}
#[test]
fn opcode_da() {
    run(0xDA);
}
#[test]
fn opcode_db() {
    run(0xDB);
}
#[test]
fn opcode_dc() {
    run(0xDC);
}
#[test]
fn opcode_dd() {
    run(0xDD);
}
#[test]
fn opcode_de() {
    run(0xDE);
}
#[test]
fn opcode_df() {
    run(0xDF);
}
#[test]
fn opcode_e0() {
    run(0xE0);
}
#[test]
fn opcode_e1() {
    run(0xE1);
}
#[test]
fn opcode_e2() {
    run(0xE2);
}
#[test]
fn opcode_e3() {
    run(0xE3);
}
#[test]
fn opcode_e4() {
    run(0xE4);
}
#[test]
fn opcode_e5() {
    run(0xE5);
}
#[test]
fn opcode_e6() {
    run(0xE6);
}
#[test]
fn opcode_e7() {
    run(0xE7);
}
#[test]
fn opcode_e8() {
    run(0xE8);
}
#[test]
fn opcode_e9() {
    run(0xE9);
}
#[test]
fn opcode_ea() {
    run(0xEA);
}
#[test]
fn opcode_eb() {
    run(0xEB);
}
#[test]
fn opcode_ec() {
    run(0xEC);
}
#[test]
fn opcode_ed() {
    run(0xED);
}
#[test]
fn opcode_ee() {
    run(0xEE);
}
#[test]
fn opcode_ef() {
    run(0xEF);
}
#[test]
fn opcode_f0() {
    run(0xF0);
}
#[test]
fn opcode_f1() {
    run(0xF1);
}
#[test]
fn opcode_f2() {
    run(0xF2);
}
#[test]
fn opcode_f3() {
    run(0xF3);
}
#[test]
fn opcode_f4() {
    run(0xF4);
}
#[test]
fn opcode_f5() {
    run(0xF5);
}
#[test]
fn opcode_f6() {
    run(0xF6);
}
#[test]
fn opcode_f7() {
    run(0xF7);
}
#[test]
fn opcode_f8() {
    run(0xF8);
}
#[test]
fn opcode_f9() {
    run(0xF9);
}
#[test]
fn opcode_fa() {
    run(0xFA);
}
#[test]
fn opcode_fb() {
    run(0xFB);
}
#[test]
fn opcode_fc() {
    run(0xFC);
}
#[test]
fn opcode_fd() {
    run(0xFD);
}
#[test]
fn opcode_fe() {
    run(0xFE);
}
#[test]
fn opcode_ff() {
    run(0xFF);
}
