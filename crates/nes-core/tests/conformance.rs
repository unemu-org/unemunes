//! The adapter against the contract's conformance list (unemu-core-api
//! `docs/conformance.md`), driven only as `&dyn Core` and `Box<dyn Machine>`
//! with the storage in memory of the `test-support` feature (item 3).
//!
//! The cartridge is built here from bytes of our own: a header, a program
//! that turns on NMI and rendering and counts frames in its NMI handler, and
//! a pattern table.

use std::rc::Rc;

use unemu_core_api::test_support::{MemoryContent, MemoryStorage};
use unemu_core_api::{
    ConfigDocument, Content, ContentSource, Core, CreateRequest, DisplayId, Error, ErrorKind,
    InputEvent, Machine, OfferedContent, Outputs, PadControl, PortId, ResetKind, RunLimit, Status,
    Stop, Ticks,
};
use unemu_nes_core::{NesConfig, NesCore, KIND, MEDIA_TYPE, PLATFORM};

const DIGEST: [u8; 32] = [0x5A; 32];

/// An NROM-128 image: at reset, NMI and rendering on, then a loop; the NMI
/// handler increments $00.
fn image(program_patch: &[(usize, u8)]) -> Vec<u8> {
    let mut prg = vec![0xEA; 0x4000];
    let program = [
        0xA9, 0x80, 0x8D, 0x00, 0x20, // LDA #$80; STA $2000
        0xA9, 0x1E, 0x8D, 0x01, 0x20, // LDA #$1E; STA $2001
        0x4C, 0x0A, 0x80, // JMP $800A
        0xE6, 0x00, // NMI: INC $00
        0x40, // RTI
    ];
    prg[..program.len()].copy_from_slice(&program);
    for &(at, byte) in program_patch {
        prg[at] = byte;
    }
    prg[0x3FFA..0x4000].copy_from_slice(&[0x0D, 0x80, 0x00, 0x80, 0x0D, 0x80]);
    let mut chr = vec![0u8; 0x2000];
    for (i, b) in chr.iter_mut().enumerate().take(16) {
        *b = if i < 8 { 0xAA } else { 0x0F };
    }
    let mut bytes = b"NES\x1A\x01\x01\x00\x00\0\0\0\0\0\0\0\0".to_vec();
    bytes.extend(prg);
    bytes.extend(chr);
    bytes
}

struct Source(Vec<u8>);

impl ContentSource for Source {
    fn open(&self, sha256: &[u8; 32], size: u64) -> Result<Content, Error> {
        if *sha256 == DIGEST && size == self.0.len() as u64 {
            Ok(Content::Bytes(self.0.clone()))
        } else {
            Err(Error::new(ErrorKind::Content, "no such content"))
        }
    }
}

fn identify(core: &dyn Core, bytes: &[u8]) -> Result<Option<unemu_core_api::Identified>, Error> {
    let reader = MemoryContent::new(bytes.to_vec());
    core.identify(&OfferedContent::new(
        "game.nes",
        DIGEST,
        bytes.len() as u64,
        &reader,
    ))
}

fn machine(core: &dyn Core, bytes: &[u8]) -> Box<dyn Machine> {
    let identified = identify(core, bytes).unwrap().expect("recognised");
    let source = Source(bytes.to_vec());
    let storage: Rc<dyn unemu_core_api::Storage> = Rc::new(MemoryStorage::new());
    let request = CreateRequest::new(PLATFORM, identified.config.as_document(), &source, storage);
    core.create(&request).expect("created")
}

#[test]
fn the_description_names_the_core_and_its_platform() {
    let core: &dyn Core = &NesCore;
    let d = core.description();
    assert_eq!((d.implements.major, d.implements.minor), (0, 6));
    assert_eq!(d.id, "unemunes");
    assert!(unemu_core_api::CONTRACT.drives(d.implements));
    let platform = &d.platforms[0];
    assert_eq!(platform.id, PLATFORM);
    assert_eq!(platform.config_types, [MEDIA_TYPE]);
    assert_eq!(platform.content[0].id, KIND);
    assert!(platform.content.iter().all(|k| k.is_valid()));
}

#[test]
fn identify_answers_none_unsupported_or_a_config() {
    let core: &dyn Core = &NesCore;
    assert!(identify(core, b"PK\x03\x04 not a cartridge")
        .unwrap()
        .is_none());
    assert!(
        identify(core, b"NES").unwrap().is_none(),
        "too short to tell"
    );
    assert_eq!(
        identify(core, b"NES\x1A\x01\x01\x00\x00").unwrap_err().kind,
        ErrorKind::Unsupported,
        "recognised but shorter than its header"
    );

    let mut truncated = image(&[]);
    truncated.truncate(5000);
    assert_eq!(
        identify(core, &truncated).unwrap_err().kind,
        ErrorKind::Unsupported
    );

    let mut mmc3 = image(&[]);
    mmc3[6] = 0x40;
    let error = identify(core, &mmc3).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert!(error.message.contains("mapper 4"), "{}", error.message);

    let good = image(&[]);
    let first = identify(core, &good).unwrap().unwrap();
    let again = identify(core, &good).unwrap().unwrap();
    assert_eq!(first.config, again.config, "same bytes, same answer");
    assert_eq!((first.platform, first.kind), (PLATFORM, KIND));
    let validated = core.validate(PLATFORM, first.config.as_document()).unwrap();
    assert_eq!(validated.content.len(), 1);
    assert_eq!(validated.content[0].sha256, DIGEST);
    assert_eq!(validated.content[0].size, good.len() as u64);
}

#[test]
fn validate_and_create_fail_alike_on_a_bad_config() {
    let core: &dyn Core = &NesCore;
    let bad = ConfigDocument::new(MEDIA_TYPE, b"cartridge.size = 1\n");
    assert_eq!(
        core.validate(PLATFORM, bad).unwrap_err().kind,
        ErrorKind::Config
    );
    let source = Source(Vec::new());
    let storage: Rc<dyn unemu_core_api::Storage> = Rc::new(MemoryStorage::new());
    let request = CreateRequest::new(PLATFORM, bad, &source, storage);
    assert_eq!(core.create(&request).err().unwrap().kind, ErrorKind::Config);
    assert_eq!(
        core.validate("pc", ConfigDocument::new(MEDIA_TYPE, b""))
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    let huge = format!(
        "cartridge.sha256 = {}\ncartridge.size = 1073741824\n",
        "5a".repeat(32)
    );
    let huge = ConfigDocument::new(MEDIA_TYPE, huge.as_bytes());
    assert_eq!(
        core.validate(PLATFORM, huge).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    let storage: Rc<dyn unemu_core_api::Storage> = Rc::new(MemoryStorage::new());
    let request = CreateRequest::new(PLATFORM, huge, &source, storage);
    assert_eq!(
        core.create(&request).err().unwrap().kind,
        ErrorKind::Unsupported
    );
}

#[test]
fn every_promised_control_works() {
    let core: &dyn Core = &NesCore;
    let mut m = machine(core, &image(&[]));
    let d = m.description();
    assert_eq!((d.clock.numerator, d.clock.denominator), (236_250_000, 11));
    assert_eq!(d.step_unit, "instruction");
    assert!(d.audio.as_ref().unwrap().rate_in_bounds());
    assert_eq!(d.inputs.len(), 2);
    let features = d.features;
    assert!(
        features.frame_stops
            && !features.saved_state
            && !features.media_changes
            && !features.media_additions
    );

    m.set_outputs(Outputs::NONE.with_audio(true));
    let mut sound = Vec::new();
    m.drain_audio(&mut sound).unwrap();
    assert_eq!(
        m.run(RunLimit::steps(1_000_000).with_stop_on_frame(true))
            .unwrap(),
        Stop::Frame(DisplayId(0))
    );
    let frame = m.frame(DisplayId(0)).unwrap();
    assert_eq!(
        (frame.width, frame.height, frame.aspect),
        (256, 240, (128, 105))
    );
    assert!(frame.visible.is_within(frame.width, frame.height));
    assert_eq!(frame.pixels.len(), 256 * 240);

    for kind in features.resets {
        m.reset(*kind).unwrap();
        m.run(RunLimit::until(Ticks(m.now().0 + 100_000))).unwrap();
    }
    m.input(InputEvent::pad(PortId(0), PadControl::Start, true))
        .unwrap();
    m.input(InputEvent::ReleaseAll).unwrap();
    m.run(RunLimit::steps(m.steps() + 10)).unwrap();
    let slots = m.slot_states();
    assert_eq!(slots[0].inserted, Some(0));
    assert_eq!(slots[0].catalogue[0].sha256, DIGEST);
    assert!(m.state_identity().is_none());
    m.finish().unwrap();
}

#[test]
fn a_deadline_is_met_at_the_next_instruction_boundary() {
    let mut m = machine(&NesCore, &image(&[]));
    for target in [1u64, 1000, 123_457, 2_000_000] {
        let until = Ticks(m.now().0.max(target));
        assert_eq!(m.run(RunLimit::until(until)).unwrap(), Stop::Deadline);
        let over = m.now().0 - until.0;
        // At most one instruction (7 cycles) plus a sprite DMA (514 cycles).
        assert!(over < 12 * (7 + 514), "{over} ticks past the deadline");
    }
    let steps = m.steps();
    assert_eq!(m.run(RunLimit::steps(steps + 5)).unwrap(), Stop::Steps);
    assert_eq!(m.steps(), steps + 5);
}

#[test]
fn sound_follows_the_position_rule() {
    let mut m = machine(&NesCore, &image(&[]));
    m.set_outputs(Outputs::NONE.with_audio(true));
    let mut sound = Vec::new();
    m.drain_audio(&mut sound).unwrap();
    for n in 1..5u64 {
        m.run(RunLimit::until(Ticks(n * 300_007))).unwrap();
        m.drain_audio(&mut sound).unwrap();
        let rate = m.description().audio.as_ref().unwrap().rate;
        let clock = m.description().clock;
        let index =
            u128::from(m.now().0) * u128::from(rate.numerator) * u128::from(clock.denominator)
                / (u128::from(rate.denominator) * u128::from(clock.numerator));
        assert_eq!(sound.len() as u128, index);
    }
    // A power cycle keeps the position rule.
    m.reset(ResetKind::Hard).unwrap();
    m.run(RunLimit::until(Ticks(m.now().0 + 4_800))).unwrap();
    m.drain_audio(&mut sound).unwrap();
    assert_eq!(sound.len() as u64, m.now().0 / 480);
    // Switched off: what was not drained is dropped at once.
    m.run(RunLimit::until(Ticks(m.now().0 + 48_000))).unwrap();
    m.set_outputs(Outputs::NONE);
    let before = sound.len();
    m.drain_audio(&mut sound).unwrap();
    assert_eq!(sound.len(), before);
    // Switched off, then on: the gap is dropped, not owed.
    m.set_outputs(Outputs::NONE);
    m.run(RunLimit::until(Ticks(m.now().0 + 500_000))).unwrap();
    m.set_outputs(Outputs::NONE.with_audio(true));
    let before = sound.len();
    m.drain_audio(&mut sound).unwrap();
    assert_eq!(sound.len(), before);
}

#[test]
fn the_same_calls_give_the_same_machine() {
    let run = || {
        let mut m = machine(&NesCore, &image(&[]));
        let mut log = Vec::new();
        m.set_outputs(Outputs::NONE.with_audio(true));
        for n in 0..5 {
            m.input(InputEvent::pad(PortId(n % 2), PadControl::A, n % 3 == 0))
                .unwrap();
            let stop = m.run(RunLimit::steps(m.steps() + 40_000).with_stop_on_frame(true));
            let frame = m.frame(DisplayId(0)).unwrap();
            let checksum = frame
                .pixels
                .iter()
                .fold(0u64, |h, &p| h.wrapping_mul(31).wrapping_add(u64::from(p)));
            let serial = frame.serial;
            let mut sound = Vec::new();
            m.drain_audio(&mut sound).unwrap();
            log.push((
                format!("{stop:?}"),
                m.now(),
                m.steps(),
                checksum,
                serial,
                sound.len(),
            ));
        }
        log
    };
    assert_eq!(run(), run());
}

#[test]
fn the_picture_serial_moves_only_when_the_pixels_do() {
    let mut m = machine(&NesCore, &image(&[]));
    let mut serials = Vec::new();
    for _ in 0..4 {
        m.run(RunLimit::steps(m.steps() + 1_000_000).with_stop_on_frame(true))
            .unwrap();
        serials.push(m.frame(DisplayId(0)).unwrap().serial);
    }
    // The program draws the same screen every frame after the first.
    assert_eq!(serials[2], serials[3]);
}

#[test]
fn an_undocumented_opcode_fails_the_run_as_a_guest_error() {
    // JMP to a $02 (JAM) at $800A instead of looping.
    let mut m = machine(&NesCore, &image(&[(10, 0x02)]));
    let error = m.run(RunLimit::steps(100)).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Guest);
    assert!(error.message.contains("$02"), "{}", error.message);
    assert_eq!(m.status(), Status::Failed);
    assert_eq!(
        m.run(RunLimit::steps(200)).unwrap_err().kind,
        ErrorKind::Usage
    );
    assert_eq!(m.finish().unwrap_err().kind, ErrorKind::Usage);
    assert!(
        m.frame(DisplayId(0)).is_ok(),
        "the last picture stays readable"
    );
}

#[test]
fn soft_reset_keeps_ram_and_restarts_the_program() {
    // The machine counts frames at $00 in RAM; a soft reset keeps the count,
    // a hard reset starts again. Observed through the picture-independent
    // run: compare step counts to the first NMI.
    let mut m = machine(&NesCore, &image(&[]));
    m.run(RunLimit::steps(1_000_000).with_stop_on_frame(true))
        .unwrap();
    m.reset(ResetKind::Soft).unwrap();
    assert!(m.run(RunLimit::steps(m.steps() + 10)).is_ok());
    let before = m.now();
    let picture = m.frame(DisplayId(0)).unwrap().pixels.to_vec();
    m.reset(ResetKind::Hard).unwrap();
    assert!(
        m.now() >= before,
        "time does not run backwards over a power cycle"
    );
    assert_eq!(
        m.frame(DisplayId(0)).unwrap().pixels,
        &picture[..],
        "the last finished picture stays until a new one is finished"
    );
}

#[test]
fn config_round_trips_through_identify() {
    let identified = identify(&NesCore, &image(&[])).unwrap().unwrap();
    let config = NesConfig::parse(identified.config.media_type, &identified.config.bytes).unwrap();
    assert_eq!(config.cartridge_sha256, DIGEST);
}

#[test]
fn unlimited_runs_accept_a_frame_stop_or_reach_a_guest_error() {
    let mut m = machine(&NesCore, &image(&[]));
    let unbounded = RunLimit::steps(0).with_steps(None);
    assert_eq!(
        m.run(unbounded.with_stop_on_frame(true)).unwrap(),
        Stop::Frame(DisplayId(0))
    );
    assert_eq!(m.status(), Status::Ready);

    let mut m = machine(&NesCore, &image(&[(0, 0x02)]));
    assert_eq!(m.run(unbounded).unwrap_err().kind, ErrorKind::Guest);
    assert_eq!(m.status(), Status::Failed);
}

#[test]
fn stops_prioritize_errors_then_requested_frames_then_deadlines_then_steps() {
    let mut m = machine(&NesCore, &image(&[(0, 0x02)]));
    // Reset consumes 84 ticks; the next opcode fetch crosses tick 85.
    assert_eq!(
        m.run(RunLimit::steps(2).with_until(Some(Ticks(85))))
            .unwrap_err()
            .kind,
        ErrorKind::Guest
    );

    let mut m = machine(&NesCore, &image(&[]));
    // First picture: 240 scanlines of 341 dots, four master ticks per dot.
    // The reset and setup take 19 CPU cycles in five steps; 9087 JMPs of
    // three cycles reach that boundary exactly, before the first NMI.
    assert_eq!(
        m.run(
            RunLimit::steps(9092)
                .with_until(Some(Ticks(327_360)))
                .with_stop_on_frame(true)
        )
        .unwrap(),
        Stop::Frame(DisplayId(0))
    );
    assert_eq!((m.now(), m.steps()), (Ticks(327_360), 9092));

    let mut m = machine(&NesCore, &image(&[]));
    assert_eq!(
        m.run(RunLimit::steps(1).with_until(Some(Ticks(84))))
            .unwrap(),
        Stop::Deadline
    );
    assert_eq!((m.now(), m.steps()), (Ticks(84), 1));
    assert_eq!(m.run(RunLimit::steps(2)).unwrap(), Stop::Steps);
    assert_eq!((m.now(), m.steps()), (Ticks(108), 2));
    assert_eq!(m.run(RunLimit::steps(1)).unwrap(), Stop::Steps);
    assert_eq!((m.now(), m.steps()), (Ticks(108), 2));
}

#[test]
fn finish_is_terminal_and_preserves_the_last_picture_and_drains() {
    let mut m = machine(&NesCore, &image(&[]));
    m.set_outputs(Outputs::NONE.with_audio(true));
    m.run(RunLimit::steps(0).with_steps(None).with_stop_on_frame(true))
        .unwrap();
    let frame = m.frame(DisplayId(0)).unwrap();
    let (pixels, serial) = (frame.pixels.to_vec(), frame.serial);
    let before = (m.now(), m.steps());
    m.finish().unwrap();
    assert_eq!(m.status(), Status::Finished);
    assert_eq!(m.finish().unwrap_err().kind, ErrorKind::Usage);
    assert_eq!(
        m.input(InputEvent::ReleaseAll).unwrap_err().kind,
        ErrorKind::Usage
    );
    assert_eq!(
        m.run(RunLimit::steps(u64::MAX)).unwrap_err().kind,
        ErrorKind::Usage
    );
    assert_eq!(m.reset(ResetKind::Soft).unwrap_err().kind, ErrorKind::Usage);
    m.set_outputs(Outputs::NONE);
    let frame = m.frame(DisplayId(0)).unwrap();
    assert_eq!((frame.pixels, frame.serial), (pixels.as_slice(), serial));
    let mut sound = Vec::new();
    m.drain_audio(&mut sound).unwrap();
    assert_eq!(sound.len(), 682);
    m.drain_messages(&mut Vec::new());
    assert_eq!(
        (m.now(), m.steps(), m.status()),
        (before.0, before.1, Status::Finished)
    );
}

#[test]
fn run_options_are_usage_errors_and_validation_stays_config_only() {
    let core: &dyn Core = &NesCore;
    let identified = identify(core, &image(&[])).unwrap().unwrap();
    // Content cannot be opened: option errors must precede content access.
    let source = Source(Vec::new());
    for options in [
        vec![("unknown", "true")],
        vec![("unknown", "true"), ("unknown", "false")],
        vec![("", "invalid")],
    ] {
        core.validate(PLATFORM, identified.config.as_document())
            .unwrap();
        let request = CreateRequest::new(
            PLATFORM,
            identified.config.as_document(),
            &source,
            Rc::new(MemoryStorage::new()),
        )
        .with_options(&options);
        assert_eq!(core.create(&request).err().unwrap().kind, ErrorKind::Usage);
    }
}
