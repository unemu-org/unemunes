//! The machine a front end drives (contract 0.5, `Machine`).
//!
//! Time: ticks are master-clock cycles at 236 250 000 / 11 Hz; a step is an
//! instruction or an interrupt or reset sequence (decision-4). A run stops at
//! the first instruction boundary at or after its deadline or step budget,
//! or after the instruction in which the PPU finished a picture when asked.
//! Input takes effect at the start of the next run, in order.
//! Lifecycle, input validation and stop priority follow unemu-core-api
//! `docs/conformance.md`, "Machine calls and statuses" and "Errors and validation".
//! Sound is silence at 984 375 / 22 Hz by the contract's position rule (`nes::apu`).

use std::collections::VecDeque;
use std::io::{Read, Write};

use nes::{apu, ppu, Buttons, Cartridge, Config, Nes};
use unemu_core_api::{
    AudioDescription, DisplayDescription, DisplayId, Error, ErrorKind, Finished, Frame, FrameArea,
    InputEvent, InputKind, InputPortDescription, Machine, MachineDescription, MediaAction,
    MediumInfo, Message, NewMedium, Outputs, PadControl, PortId, Rate, ResetKind, RunLimit,
    SlotDescription, SlotId, SlotKind, SlotState, Status, Stop, Ticks, WriteKind,
};
use x65::Event;

use crate::config::NesConfig;
use crate::{FEATURES, KIND};

/// The NTSC master clock.
const CLOCK: Rate = Rate::new(236_250_000, 11);
/// The picture's shape: 256 × 240 pixels of 8:7, so (256 × 8) : (240 × 7).
const ASPECT: (u32, u32) = (128, 105);
/// The centre 256 × 224 that NTSC TVs showed (NESdev wiki, "Overscan",
/// revision 22760).
const VISIBLE: FrameArea = FrameArea::new(0, 8, 256, 224);
const DISPLAY: DisplayId = DisplayId(0);
const CARTRIDGE: SlotId = SlotId(0);
/// A pad's controls, as the standard controller has them.
const CONTROLS: [PadControl; 8] = [
    PadControl::Up,
    PadControl::Down,
    PadControl::Left,
    PadControl::Right,
    PadControl::A,
    PadControl::B,
    PadControl::Select,
    PadControl::Start,
];

/// A running NES.
pub struct NesMachine {
    nes: Nes,
    cartridge: Cartridge,
    power_on: Config,
    config: NesConfig,
    description: MachineDescription,
    status: Status,
    steps: u64,
    /// Absolute master ticks, checked at each instruction boundary.
    ticks: u64,
    pending: VecDeque<InputEvent>,
    buttons: [Buttons; 2],
    outputs: Outputs,
    messages: Vec<Message>,
    pixels: Vec<u32>,
    scratch: Vec<u32>,
    frames_finished: u64,
    pixels_frame: u64,
    serial: u64,
    /// The index of the next sound frame to hand over, counted from the
    /// machine's creation, so a power cycle does not restart it.
    audio_next: u64,
}

impl NesMachine {
    pub(crate) fn new(
        cartridge: Cartridge,
        power_on: Config,
        config: NesConfig,
    ) -> Result<Self, Error> {
        let nes = Nes::new(&cartridge, power_on)
            .map_err(|error| Error::new(ErrorKind::Unsupported, error.to_string()))?;
        let pad = |n: u8| {
            InputPortDescription::new(
                PortId(n),
                InputKind::pad(CONTROLS.to_vec()),
                format!("Controller {}", n + 1),
            )
        };
        let description = MachineDescription::new(CLOCK, "instruction")
            .with_displays(vec![DisplayDescription::new(DISPLAY, "Picture")])
            .with_audio(Some(AudioDescription::new(Rate::new(
                apu::RATE.0,
                apu::RATE.1,
            ))))
            .with_inputs(vec![pad(0), pad(1)])
            .with_slots(vec![SlotDescription::new(
                CARTRIDGE,
                SlotKind::Cartridge,
                "Cartridge",
                false,
            )
            .with_formats(vec![KIND.to_string()])])
            .with_features(FEATURES);
        Ok(NesMachine {
            nes,
            cartridge,
            power_on,
            config,
            description,
            status: Status::Ready,
            steps: 0,
            ticks: 0,
            pending: VecDeque::new(),
            buttons: [Buttons::default(); 2],
            outputs: Outputs::NONE,
            messages: Vec::new(),
            pixels: vec![0; ppu::WIDTH * ppu::HEIGHT],
            scratch: Vec::new(),
            frames_finished: 0,
            pixels_frame: 0,
            serial: 0,
            audio_next: 0,
        })
    }

    fn master(&self) -> u64 {
        self.ticks
    }

    fn apply_input(&mut self) {
        while let Some(event) = self.pending.pop_front() {
            match event {
                InputEvent::Pad {
                    port,
                    control,
                    pressed,
                    ..
                } if port.0 < 2 => {
                    let bit = match control {
                        PadControl::A => Buttons::A,
                        PadControl::B => Buttons::B,
                        PadControl::Select => Buttons::SELECT,
                        PadControl::Start => Buttons::START,
                        PadControl::Up => Buttons::UP,
                        PadControl::Down => Buttons::DOWN,
                        PadControl::Left => Buttons::LEFT,
                        PadControl::Right => Buttons::RIGHT,
                        _ => continue,
                    };
                    let held = &mut self.buttons[usize::from(port.0)];
                    held.0 = if pressed {
                        held.0 | bit.0
                    } else {
                        held.0 & !bit.0
                    };
                }
                InputEvent::ReleaseAll => self.buttons = [Buttons::default(); 2],
                _ => {}
            }
        }
        for (port, buttons) in self.buttons.iter().enumerate() {
            self.nes.set_buttons(port, *buttons);
        }
    }

    /// Drop the sound up to now.
    fn skip_audio(&mut self) {
        self.audio_next = self.master() / apu::MASTER_PER_FRAME;
    }

    /// Reject terminal mutations before capability and argument checks (contract 0.4).
    fn live(&self) -> Result<(), Error> {
        if matches!(self.status, Status::Failed | Status::Finished) {
            Err(Error::new(ErrorKind::Usage, "the machine is terminal"))
        } else {
            Ok(())
        }
    }

    /// Input and media are also unavailable while powered off (contract 0.4).
    fn interactive(&self) -> Result<(), Error> {
        self.live()?;
        if self.status == Status::Off {
            Err(Error::new(ErrorKind::Usage, "the machine is powered off"))
        } else {
            Ok(())
        }
    }

    fn fail(&mut self, error: Error) -> Error {
        self.status = Status::Failed;
        if !self.outputs.audio {
            self.skip_audio();
        }
        error
    }
}

impl Machine for NesMachine {
    fn description(&self) -> &MachineDescription {
        &self.description
    }

    fn now(&self) -> Ticks {
        Ticks(self.master())
    }

    fn steps(&self) -> u64 {
        self.steps
    }

    fn status(&self) -> Status {
        self.status
    }

    fn run(&mut self, limit: RunLimit) -> Result<Stop, Error> {
        self.live()?;
        match self.status {
            Status::Halted => return Ok(Stop::Halted),
            Status::Off => return Ok(Stop::PoweredOff),
            _ => {}
        }
        if limit.stop_on_frame && !self.description.features.frame_stops {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "frame stops unavailable",
            ));
        }
        self.apply_input();
        let stop = loop {
            if limit.until.is_some_and(|until| self.master() >= until.0) {
                break Stop::Deadline;
            }
            if limit.steps.is_some_and(|steps| self.steps >= steps) {
                break Stop::Steps;
            }
            let Some(steps) = self.steps.checked_add(1) else {
                return Err(self.fail(Error::new(ErrorKind::Guest, "step counter exhausted")));
            };
            let before = self.nes.cpu_cycles();
            let event = self.nes.step();
            // Account only for this step, avoiding an unchecked conversion of
            // the console's lifetime CPU cycles to absolute master ticks.
            let ticks = (self.nes.cpu_cycles() - before)
                .checked_mul(nes::machine::MASTER_PER_CPU)
                .and_then(|elapsed| self.ticks.checked_add(elapsed));
            let Some(ticks) = ticks else {
                return Err(self.fail(Error::new(ErrorKind::Guest, "tick counter exhausted")));
            };
            self.ticks = ticks;
            match event {
                Event::Unsupported { opcode, .. } => {
                    let pc = self.nes.cpu().pc;
                    return Err(self.fail(Error::new(
                        ErrorKind::Guest,
                        format!(
                            "the program reached opcode ${opcode:02X} at ${pc:04X}, an undocumented instruction this core does not implement yet"
                        ),
                    )));
                }
                _ => self.steps = steps,
            }
            if self.nes.take_frame_done() {
                self.frames_finished += 1;
                if limit.stop_on_frame {
                    break Stop::Frame(DISPLAY);
                }
            }
        };
        if !self.outputs.audio {
            self.skip_audio();
        }
        Ok(stop)
    }

    fn frame(&mut self, display: DisplayId) -> Result<Frame<'_>, Error> {
        if display != DISPLAY {
            return Err(Error::new(ErrorKind::Usage, "this machine has one display"));
        }
        if self.pixels_frame != self.frames_finished {
            self.nes.picture_rgb(&mut self.scratch);
            if self.scratch != self.pixels {
                std::mem::swap(&mut self.scratch, &mut self.pixels);
                self.serial += 1;
            }
            self.pixels_frame = self.frames_finished;
        }
        Ok(Frame::new(
            ppu::WIDTH as u32,
            ppu::HEIGHT as u32,
            &self.pixels,
            ASPECT,
            self.serial,
        )
        .with_visible(VISIBLE))
    }

    fn drain_audio(&mut self, out: &mut Vec<[i16; 2]>) -> Result<(), Error> {
        // The APU is silent (`nes::apu`): frame i is silence for every i.
        let until = self.master() / apu::MASTER_PER_FRAME;
        if until > self.audio_next {
            out.extend(core::iter::repeat_n(
                [0, 0],
                (until - self.audio_next) as usize,
            ));
            self.audio_next = until;
        }
        Ok(())
    }

    fn set_outputs(&mut self, outputs: Outputs) {
        if self.live().is_err() {
            return;
        }
        if outputs.audio != self.outputs.audio {
            // Sound not drained when it is switched off, and sound made
            // while it was off, is dropped, not owed (conformance item 10).
            self.skip_audio();
        }
        self.outputs = outputs;
    }

    fn input(&mut self, event: InputEvent) -> Result<(), Error> {
        self.interactive()?;
        let valid = match &event {
            InputEvent::ReleaseAll => true,
            InputEvent::Pad { port, control, .. } => port.0 < 2 && CONTROLS.contains(control),
            InputEvent::Key { .. }
            | InputEvent::PointerMotion { .. }
            | InputEvent::PointerButton { .. }
            | InputEvent::PointerPosition { .. } => false,
            _ => {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "unknown input operation",
                ))
            }
        };
        if !valid {
            return Err(Error::new(
                ErrorKind::Usage,
                "invalid input port or control",
            ));
        }
        self.pending.push_back(event);
        Ok(())
    }

    fn slot_states(&mut self) -> Vec<SlotState> {
        vec![SlotState::new(CARTRIDGE)
            .with_catalogue(vec![MediumInfo::new(
                "Cartridge",
                self.config.cartridge_sha256,
                self.config.cartridge_size,
                KIND,
                WriteKind::ReadOnly,
            )])
            .with_inserted(Some(0))
            .with_accepts_changes(false)]
    }

    fn add_medium(&mut self, _: SlotId, _: NewMedium<'_>) -> Result<usize, Error> {
        self.interactive()?;
        Err(Error::new(
            ErrorKind::Unsupported,
            "the cartridge slot does not change while the machine runs",
        ))
    }

    fn change_medium(&mut self, _: SlotId, _: MediaAction) -> Result<(), Error> {
        self.interactive()?;
        Err(Error::new(
            ErrorKind::Unsupported,
            "the cartridge slot does not change while the machine runs",
        ))
    }

    fn reset(&mut self, kind: ResetKind) -> Result<(), Error> {
        self.live()?;
        if self.status == Status::Off && kind != ResetKind::Hard {
            return Err(Error::new(
                ErrorKind::Usage,
                "only hard reset can restart power",
            ));
        }
        match kind {
            ResetKind::Soft => self.nes.press_reset(),
            ResetKind::Hard => {
                let nes = Nes::new(&self.cartridge, self.power_on)
                    .map_err(|error| Error::new(ErrorKind::Unsupported, error.to_string()))?;
                // Time, the sound position and the last finished picture
                // carry over the power cycle.
                self.nes = nes;
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "a reset this machine does not have",
                ))
            }
        }
        self.pending.clear();
        self.buttons = [Buttons::default(); 2];
        for port in 0..2 {
            self.nes.set_buttons(port, Buttons::default());
        }
        self.status = Status::Ready;
        Ok(())
    }

    fn state_identity(&self) -> Option<String> {
        None
    }

    fn save_state(&mut self, _: &mut dyn Write) -> Result<(), Error> {
        self.live()?;
        Err(Error::new(
            ErrorKind::Unsupported,
            "saved state comes in milestone N3",
        ))
    }

    fn load_state(&mut self, _: &mut dyn Read) -> Result<(), Error> {
        self.live()?;
        Err(Error::new(
            ErrorKind::Unsupported,
            "saved state comes in milestone N3",
        ))
    }

    fn finish(&mut self) -> Result<Finished, Error> {
        self.live()?;
        // Nothing to make durable: no battery memory in the proof slice.
        self.status = Status::Finished;
        Ok(Finished::new())
    }

    fn drain_messages(&mut self, out: &mut Vec<Message>) {
        out.append(&mut self.messages);
    }
}

#[cfg(test)]
mod tests {
    //! Contract 0.4 `docs/conformance.md`, call/status and error matrices.
    //! Private status injection covers Halted/Off, which this NES cannot emit.
    use super::*;
    use unemu_core_api::{Content, Key, KeyState, WriteTarget};

    fn machine() -> NesMachine {
        // Own program: hold the strobe high, sample A from both controllers
        // into RAM, and repeat (NESdev wiki, "Standard controller", rev 23466).
        let program = [
            0xA9, 0x01, 0x8D, 0x16, 0x40, // LDA #1; STA $4016
            0xAD, 0x16, 0x40, 0x29, 0x01, 0x85, 0x00, // LDA $4016; AND #1; STA $00
            0xAD, 0x17, 0x40, 0x29, 0x01, 0x85, 0x01, // LDA $4017; AND #1; STA $01
            0x4C, 0x00, 0x80, // JMP $8000
        ];
        let mut bytes = b"NES\x1A\x01\x01\x00\x00\0\0\0\0\0\0\0\0".to_vec();
        let mut prg = vec![0xEA; 0x4000];
        prg[..program.len()].copy_from_slice(&program);
        prg[0x3FFA..].copy_from_slice(&[0x00, 0x80, 0x00, 0x80, 0x00, 0x80]);
        bytes.extend(prg);
        bytes.extend(vec![0; 0x2000]);
        NesMachine::new(
            Cartridge::from_ines(&bytes).unwrap(),
            Config::default(),
            NesConfig::for_cartridge([0; 32], bytes.len() as u64),
        )
        .unwrap()
    }

    #[test]
    fn input_checks_status_before_arguments_and_preserves_the_queue() {
        for status in [Status::Off, Status::Failed, Status::Finished] {
            let mut m = machine();
            m.input(InputEvent::pad(PortId(0), PadControl::A, true))
                .unwrap();
            m.status = status;
            for event in [
                InputEvent::ReleaseAll,
                InputEvent::pad(PortId(0), PadControl::A, false),
                InputEvent::pad(PortId(255), PadControl::X, true),
            ] {
                assert_eq!(m.input(event).unwrap_err().kind, ErrorKind::Usage);
                assert_eq!(m.status(), status);
                assert_eq!(m.pending, [InputEvent::pad(PortId(0), PadControl::A, true)]);
            }
        }
    }

    #[test]
    fn input_checks_ports_kinds_and_advertised_controls_before_queuing() {
        for status in [Status::Ready, Status::Halted] {
            let mut m = machine();
            m.status = status;
            m.input(InputEvent::pad(PortId(1), PadControl::Start, true))
                .unwrap();
            for event in [
                InputEvent::pad(PortId(2), PadControl::A, true),
                InputEvent::pad(PortId(255), PadControl::A, true),
                InputEvent::pad(PortId(0), PadControl::X, true),
                InputEvent::pad(PortId(0), PadControl::Y, true),
                InputEvent::pad(PortId(1), PadControl::L, true),
                InputEvent::pad(PortId(1), PadControl::R, true),
                InputEvent::key(PortId(0), Key(4), KeyState::Pressed),
                InputEvent::pointer_motion(PortId(0), 1, 2),
                InputEvent::pointer_button(PortId(0), 0, true),
                InputEvent::pointer_position(PortId(0), Some((1, 2))),
            ] {
                assert_eq!(m.input(event).unwrap_err().kind, ErrorKind::Usage);
                assert_eq!(m.status(), status);
                assert_eq!(
                    m.pending,
                    [InputEvent::pad(PortId(1), PadControl::Start, true)]
                );
            }
            m.input(InputEvent::ReleaseAll).unwrap();
            assert_eq!(m.pending.len(), 2);
        }
    }

    #[test]
    fn halted_and_off_run_do_not_validate_or_consume_input() {
        for (status, stop) in [
            (Status::Halted, Stop::Halted),
            (Status::Off, Stop::PoweredOff),
        ] {
            let mut m = machine();
            m.input(InputEvent::pad(PortId(0), PadControl::A, true))
                .unwrap();
            m.status = status;
            m.description.features.frame_stops = false;
            for limit in [
                RunLimit::steps(0).with_steps(None),
                RunLimit::steps(0).with_stop_on_frame(true),
            ] {
                assert_eq!(m.run(limit).unwrap(), stop);
                assert_eq!((m.now(), m.steps(), m.status()), (Ticks(0), 0, status));
                assert_eq!(m.pending, [InputEvent::pad(PortId(0), PadControl::A, true)]);
                assert_eq!(m.buttons, [Buttons(0), Buttons(0)]);
                assert_eq!(m.nes.cpu_cycles(), 0);
            }
        }
    }

    #[test]
    fn unsupported_frame_stop_preserves_input() {
        let mut m = machine();
        m.description.features.frame_stops = false;
        m.input(InputEvent::ReleaseAll).unwrap();
        assert_eq!(
            m.run(RunLimit::steps(0).with_steps(None).with_stop_on_frame(true))
                .unwrap_err()
                .kind,
            ErrorKind::Unsupported
        );
        assert_eq!(m.pending, [InputEvent::ReleaseAll]);
        assert_eq!(
            (m.now(), m.steps(), m.status()),
            (Ticks(0), 0, Status::Ready)
        );
    }

    #[test]
    fn reached_limits_apply_input_in_order_without_execution() {
        for (limit, expected) in [
            (RunLimit::steps(0), Stop::Steps),
            (RunLimit::until(Ticks(0)), Stop::Deadline),
            (
                RunLimit::steps(0).with_until(Some(Ticks(0))),
                Stop::Deadline,
            ),
        ] {
            let mut m = machine();
            m.input(InputEvent::pad(PortId(0), PadControl::A, true))
                .unwrap();
            m.input(InputEvent::ReleaseAll).unwrap();
            m.input(InputEvent::pad(PortId(1), PadControl::B, true))
                .unwrap();
            let stop = m.run(limit).unwrap();
            assert_eq!(stop, expected);
            assert_eq!(m.buttons, [Buttons(0), Buttons(2)]);
            assert!(m.pending.is_empty());
            assert_eq!((m.now(), m.steps()), (Ticks(0), 0));
            assert_eq!(m.nes.cpu_cycles(), 0);
        }
    }

    #[test]
    fn resets_clear_queued_and_held_controls_without_taking_time() {
        for kind in [ResetKind::Soft, ResetKind::Hard] {
            let mut m = machine();
            m.input(InputEvent::pad(PortId(0), PadControl::A, true))
                .unwrap();
            m.input(InputEvent::pad(PortId(1), PadControl::A, true))
                .unwrap();
            m.run(RunLimit::steps(12)).unwrap();
            assert_eq!(&m.nes.ram()[..2], &[1, 1]);
            let before = (m.now(), m.steps());
            m.input(InputEvent::pad(PortId(0), PadControl::B, true))
                .unwrap();
            m.status = Status::Halted;
            m.reset(kind).unwrap();
            assert_eq!((m.now(), m.steps()), before);
            assert_eq!(m.status(), Status::Ready);
            assert!(m.pending.is_empty());
            assert_eq!(m.buttons, [Buttons(0), Buttons(0)]);
            // Bypass run's input application to witness reset clearing the
            // console's held controls as well as the adapter's copy.
            for _ in 0..12 {
                m.nes.step();
            }
            assert_eq!(&m.nes.ram()[..2], &[0, 0]);
        }
    }

    #[test]
    fn only_hard_reset_restarts_off_and_soft_rejection_preserves_input() {
        let mut m = machine();
        m.run(RunLimit::steps(2)).unwrap();
        assert_eq!((m.now(), m.steps()), (Ticks(108), 2));
        m.input(InputEvent::ReleaseAll).unwrap();
        m.status = Status::Off;
        assert_eq!(m.reset(ResetKind::Soft).unwrap_err().kind, ErrorKind::Usage);
        assert_eq!(
            (m.now(), m.steps(), m.status()),
            (Ticks(108), 2, Status::Off)
        );
        assert_eq!(m.pending, [InputEvent::ReleaseAll]);
        m.reset(ResetKind::Hard).unwrap();
        assert_eq!(
            (m.now(), m.steps(), m.status()),
            (Ticks(108), 2, Status::Ready)
        );
        assert!(m.pending.is_empty());
    }

    #[test]
    fn terminal_guards_precede_capability_checks_and_preserve_observations() {
        for status in [Status::Failed, Status::Finished] {
            let mut m = machine();
            m.set_outputs(Outputs::NONE.with_audio(true));
            m.run(RunLimit::steps(100)).unwrap();
            let before = (m.now(), m.steps());
            let picture = m.frame(DISPLAY).unwrap().pixels.to_vec();
            m.status = status;
            m.set_outputs(Outputs::NONE);
            assert!(m.outputs.audio);
            assert_eq!(
                m.run(RunLimit::steps(0).with_steps(None)).unwrap_err().kind,
                ErrorKind::Usage
            );
            assert_eq!(m.reset(ResetKind::Hard).unwrap_err().kind, ErrorKind::Usage);
            assert_eq!(
                m.save_state(&mut Vec::new()).unwrap_err().kind,
                ErrorKind::Usage
            );
            assert_eq!(
                m.load_state(&mut &b"bad state"[..]).unwrap_err().kind,
                ErrorKind::Usage
            );
            assert_eq!(m.finish().unwrap_err().kind, ErrorKind::Usage);
            assert_eq!(
                m.change_medium(SlotId(255), MediaAction::Insert(255))
                    .unwrap_err()
                    .kind,
                ErrorKind::Usage
            );
            let medium = NewMedium::new(
                "bad",
                Content::Bytes(Vec::new()),
                [0; 32],
                0,
                "bad",
                WriteTarget::ReadOnly,
            );
            assert_eq!(
                m.add_medium(SlotId(255), medium).unwrap_err().kind,
                ErrorKind::Usage
            );
            assert_eq!(m.frame(DISPLAY).unwrap().pixels, picture);
            assert_eq!(m.frame(DisplayId(255)).unwrap_err().kind, ErrorKind::Usage);
            assert_eq!(m.slot_states()[0].inserted, Some(0));
            assert!(m.state_identity().is_none());
            let mut audio = Vec::new();
            m.drain_audio(&mut audio).unwrap();
            assert!(!audio.is_empty());
            m.drain_messages(&mut Vec::new());
            assert_eq!(
                (m.now(), m.steps(), m.status()),
                (before.0, before.1, status)
            );
        }
    }

    #[test]
    fn missing_capabilities_precede_invalid_arguments_except_in_off() {
        for status in [Status::Ready, Status::Halted, Status::Off] {
            let mut m = machine();
            m.status = status;
            let media_error = if status == Status::Off {
                ErrorKind::Usage
            } else {
                ErrorKind::Unsupported
            };
            assert_eq!(
                m.change_medium(SlotId(255), MediaAction::Insert(255))
                    .unwrap_err()
                    .kind,
                media_error
            );
            let medium = NewMedium::new(
                "bad",
                Content::Bytes(Vec::new()),
                [0; 32],
                0,
                "bad",
                WriteTarget::ReadOnly,
            );
            assert_eq!(
                m.add_medium(SlotId(255), medium).unwrap_err().kind,
                media_error
            );
            assert_eq!(
                m.save_state(&mut Vec::new()).unwrap_err().kind,
                ErrorKind::Unsupported
            );
            assert_eq!(
                m.load_state(&mut &b"bad state"[..]).unwrap_err().kind,
                ErrorKind::Unsupported
            );
            assert_eq!(m.status(), status);
        }
    }

    #[test]
    fn counters_fail_without_wrapping_and_reached_limits_win() {
        let mut m = machine();
        m.steps = u64::MAX;
        assert_eq!(m.run(RunLimit::steps(u64::MAX)).unwrap(), Stop::Steps);
        assert_eq!(
            m.run(RunLimit::steps(0).with_steps(None)).unwrap_err().kind,
            ErrorKind::Guest
        );
        assert_eq!(
            (m.steps(), m.now(), m.status()),
            (u64::MAX, Ticks(0), Status::Failed)
        );
        assert_eq!(m.nes.cpu_cycles(), 0);

        let mut m = machine();
        // The initial reset takes 84 ticks: it still fits exactly here.
        m.ticks = u64::MAX - 84;
        m.run(RunLimit::steps(1)).unwrap();
        assert_eq!((m.now(), m.steps()), (Ticks(u64::MAX), 1));
        assert_eq!(
            m.run(RunLimit::until(Ticks(u64::MAX))).unwrap(),
            Stop::Deadline
        );
        assert_eq!(
            m.run(RunLimit::steps(0).with_steps(None)).unwrap_err().kind,
            ErrorKind::Guest
        );
        assert_eq!(
            (m.now(), m.steps(), m.status()),
            (Ticks(u64::MAX), 1, Status::Failed)
        );
    }
}
