//! The machine a front end drives (contract 0.2, `Machine`).
//!
//! Time: ticks are master-clock cycles at 236 250 000 / 11 Hz; a step is an
//! instruction or an interrupt or reset sequence (decision-4). A run stops at
//! the first instruction boundary at or after its deadline or step budget,
//! or after the instruction in which the PPU finished a picture when asked.
//! Input takes effect at the start of the next run, in order. Sound is
//! silence at 984 375 / 22 Hz by the contract's position rule (`nes::apu`).

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
const VISIBLE: FrameArea = FrameArea {
    x: 0,
    y: 8,
    width: 256,
    height: 224,
};
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
    /// Master cycles of the consoles before the last power cycle.
    base: u64,
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
                InputKind::Pad {
                    controls: CONTROLS.to_vec(),
                },
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
            base: 0,
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
        self.base + self.nes.master_cycles()
    }

    fn apply_input(&mut self) {
        while let Some(event) = self.pending.pop_front() {
            match event {
                InputEvent::Pad {
                    port,
                    control,
                    pressed,
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

    fn fail(&mut self, error: Error) -> Error {
        self.status = Status::Failed;
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
        if self.status != Status::Ready {
            return Err(Error::new(ErrorKind::Usage, "the machine cannot run now"));
        }
        if limit.until.is_none() && limit.steps.is_none() && !limit.stop_on_frame {
            return Err(Error::new(ErrorKind::Usage, "a run needs a limit"));
        }
        self.apply_input();
        let stop = loop {
            if limit.until.is_some_and(|until| self.master() >= until.0) {
                break Stop::Deadline;
            }
            if limit.steps.is_some_and(|steps| self.steps >= steps) {
                break Stop::Steps;
            }
            match self.nes.step() {
                Event::Unsupported { opcode } => {
                    let pc = self.nes.cpu().pc;
                    return Err(self.fail(Error::new(
                        ErrorKind::Guest,
                        format!(
                            "the program reached opcode ${opcode:02X} at ${pc:04X}, an undocumented instruction this core does not implement yet"
                        ),
                    )));
                }
                _ => self.steps += 1,
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
        if outputs.audio != self.outputs.audio {
            // Sound not drained when it is switched off, and sound made
            // while it was off, is dropped, not owed (conformance item 10).
            self.skip_audio();
        }
        self.outputs = outputs;
    }

    fn input(&mut self, event: InputEvent) {
        self.pending.push_back(event);
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
        Err(Error::new(
            ErrorKind::Unsupported,
            "the cartridge slot does not change while the machine runs",
        ))
    }

    fn change_medium(&mut self, _: SlotId, _: MediaAction) -> Result<(), Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "the cartridge slot does not change while the machine runs",
        ))
    }

    fn reset(&mut self, kind: ResetKind) -> Result<(), Error> {
        if self.status == Status::Failed {
            return Err(Error::new(ErrorKind::Usage, "the machine failed"));
        }
        match kind {
            ResetKind::Soft => self.nes.press_reset(),
            ResetKind::Hard => {
                let nes = Nes::new(&self.cartridge, self.power_on)
                    .map_err(|error| Error::new(ErrorKind::Unsupported, error.to_string()))?;
                // Time, the sound position and the last finished picture
                // carry over the power cycle.
                self.base += self.nes.master_cycles();
                self.nes = nes;
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "a reset this machine does not have",
                ))
            }
        }
        Ok(())
    }

    fn state_identity(&self) -> Option<String> {
        None
    }

    fn save_state(&mut self, _: &mut dyn Write) -> Result<(), Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "saved state comes in milestone N3",
        ))
    }

    fn load_state(&mut self, _: &mut dyn Read) -> Result<(), Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "saved state comes in milestone N3",
        ))
    }

    fn finish(&mut self) -> Result<Finished, Error> {
        if self.status == Status::Failed {
            return Err(Error::new(ErrorKind::Usage, "the machine failed"));
        }
        // Nothing to make durable: no battery memory in the proof slice.
        Ok(Finished::new())
    }

    fn drain_messages(&mut self, out: &mut Vec<Message>) {
        out.append(&mut self.messages);
    }
}
