//! Witnesses for the reset sequence, NMI and IRQ, and when the processor
//! takes them.
//!
//! Expected traces come from the NESdev wiki's "CPU interrupts" (revision
//! 21632), whose tick-by-tick tables are derived from the Visual 6502
//! transistor-level simulation, and "CPU power up state" (revision 22091),
//! measured on an RP2A03G; the push order is the one SingleStepTests show
//! for BRK. The exact polling inside branches and NMI hijacking BRK are
//! milestone N1's, with blargg's `cpu_interrupts_v2` as their hardware
//! witness.

use x65::{flags, Bus, Cpu, Event, Interrupt, Lines, Model};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Read(u16),
    Write(u16, u8),
}

/// 64 KiB of RAM, a log of accesses, and input lines the test sets.
struct Machine {
    memory: Vec<u8>,
    log: Vec<Access>,
    lines: Lines,
}

impl Machine {
    fn new() -> Self {
        Machine {
            memory: vec![0; 0x10000],
            log: Vec::new(),
            lines: Lines::default(),
        }
    }

    fn load(&mut self, address: u16, bytes: &[u8]) {
        let start = usize::from(address);
        self.memory[start..start + bytes.len()].copy_from_slice(bytes);
    }

    fn vectors(&mut self, nmi: u16, reset: u16, irq: u16) {
        self.load(0xFFFA, &nmi.to_le_bytes());
        self.load(0xFFFC, &reset.to_le_bytes());
        self.load(0xFFFE, &irq.to_le_bytes());
    }
}

impl Bus for Machine {
    fn read(&mut self, address: u16) -> u8 {
        self.log.push(Access::Read(address));
        self.memory[usize::from(address)]
    }
    fn write(&mut self, address: u16, value: u8) {
        self.log.push(Access::Write(address, value));
        self.memory[usize::from(address)] = value;
    }
    fn lines(&mut self) -> Lines {
        self.lines
    }
}

/// A powered-on processor that has run its reset sequence into `start`.
fn booted(machine: &mut Machine, start: u16) -> Cpu {
    machine.vectors(0x9000, start, 0xA000);
    let mut cpu = Cpu::new(Model::Rp2a03);
    assert_eq!(cpu.step(machine), Event::Reset);
    machine.log.clear();
    cpu
}

#[test]
fn power_on_runs_the_reset_sequence_without_writing() {
    let mut machine = Machine::new();
    machine.vectors(0x9000, 0x8000, 0xA000);
    let mut cpu = Cpu::new(Model::Rp2a03);

    assert_eq!(cpu.step(&mut machine), Event::Reset);

    // Two reads at PC, three stack reads while S drops by 3 (writes are
    // suppressed), then the reset vector.
    assert_eq!(
        machine.log,
        [
            Access::Read(0x0000),
            Access::Read(0x0000),
            Access::Read(0x0100),
            Access::Read(0x01FF),
            Access::Read(0x01FE),
            Access::Read(0xFFFC),
            Access::Read(0xFFFD),
        ]
    );
    let state = cpu.state();
    assert_eq!(state.pc, 0x8000);
    assert_eq!(state.s, 0xFD, "S = $00 - 3 (CPU power up state)");
    assert_ne!(state.p & flags::INTERRUPT_DISABLE, 0);
    assert_eq!((state.a, state.x, state.y), (0, 0, 0));
}

#[test]
fn reset_line_resets_at_the_next_boundary_and_keeps_memory() {
    let mut machine = Machine::new();
    machine.load(0x8000, &[0xEA, 0xEA]); // NOP, NOP
    let mut cpu = booted(&mut machine, 0x8000);
    machine.memory[0x01FD] = 0x55;

    machine.lines.reset = true;
    assert_eq!(cpu.step(&mut machine), Event::Instruction { opcode: 0xEA });
    machine.lines.reset = false;
    assert_eq!(cpu.step(&mut machine), Event::Reset);

    assert_eq!(cpu.state().pc, 0x8000);
    assert_eq!(cpu.state().s, 0xFA, "S drops by 3 again");
    assert_eq!(machine.memory[0x01FD], 0x55, "a reset writes nothing");
    assert!(machine.log.iter().all(|a| matches!(a, Access::Read(_))));
}

/// Hardware Manual §1.4.1.2.11: while RES is low writing is inhibited; when
/// the line goes high the processor runs the reset sequence and fetches the
/// vector. One press is one sequence, however long it lasts.
#[test]
fn held_reset_makes_no_progress_and_resets_once_on_release() {
    let mut machine = Machine::new();
    machine.load(0x8000, &[0xEA, 0xEA, 0xEA]);
    let mut cpu = booted(&mut machine, 0x8000);
    machine.memory[0x01FD] = 0x55;

    machine.lines.reset = true;
    assert_eq!(cpu.step(&mut machine), Event::Instruction { opcode: 0xEA });
    for _ in 0..100 {
        assert_eq!(cpu.tick(&mut machine), None, "no progress while held");
    }
    assert_eq!(cpu.state().pc, 0x8001);
    assert_eq!(cpu.state().s, 0xFD);

    machine.lines.reset = false;
    let mut events = Vec::new();
    while events.len() < 3 {
        if let Some(event) = cpu.tick(&mut machine) {
            events.push(event);
        }
    }
    assert_eq!(
        events,
        [
            Event::Reset,
            Event::Instruction { opcode: 0xEA },
            Event::Instruction { opcode: 0xEA },
        ]
    );
    assert_eq!(cpu.state().s, 0xFA, "S drops by 3 once");
    assert_eq!(machine.memory[0x01FD], 0x55);
    assert!(machine.log.iter().all(|a| matches!(a, Access::Read(_))));
}

#[test]
fn nmi_pushes_pc_and_p_with_b_clear_and_jumps_through_fffa() {
    let mut machine = Machine::new();
    machine.load(0x8000, &[0xEA, 0xEA]);
    let mut cpu = booted(&mut machine, 0x8000);
    let p_before = cpu.state().p;

    machine.lines.nmi = true;
    assert_eq!(cpu.step(&mut machine), Event::Instruction { opcode: 0xEA });
    machine.log.clear();
    assert_eq!(cpu.step(&mut machine), Event::Interrupt(Interrupt::Nmi));

    assert_eq!(
        machine.log,
        [
            Access::Read(0x8001),
            Access::Read(0x8001),
            Access::Write(0x01FD, 0x80),
            Access::Write(0x01FC, 0x01),
            Access::Write(0x01FB, p_before & !flags::BREAK | flags::UNUSED),
            Access::Read(0xFFFA),
            Access::Read(0xFFFB),
        ]
    );
    assert_eq!(cpu.state().pc, 0x9000);
    assert_ne!(cpu.state().p & flags::INTERRUPT_DISABLE, 0);
}

#[test]
fn nmi_is_an_edge_held_line_triggers_once() {
    let mut machine = Machine::new();
    machine.load(0x8000, &[0xEA, 0xEA]);
    machine.load(0x9000, &[0xEA, 0xEA, 0xEA]);
    let mut cpu = booted(&mut machine, 0x8000);

    machine.lines.nmi = true;
    cpu.step(&mut machine);
    assert_eq!(cpu.step(&mut machine), Event::Interrupt(Interrupt::Nmi));
    // The line stays asserted: no second NMI without a new edge.
    for _ in 0..3 {
        assert_eq!(cpu.step(&mut machine), Event::Instruction { opcode: 0xEA });
    }
}

#[test]
fn irq_is_masked_by_i_and_taken_when_clear() {
    let mut machine = Machine::new();
    // SEI is the power-on state; NOP; CLI; NOP; NOP.
    machine.load(0x8000, &[0xEA, 0x58, 0xEA, 0xEA]);
    machine.load(0xA000, &[0xEA]);
    let mut cpu = booted(&mut machine, 0x8000);

    machine.lines.irq = true;
    assert_eq!(cpu.step(&mut machine), Event::Instruction { opcode: 0xEA });
    // CLI clears I after the poll, so one more instruction runs first
    // ("Delayed IRQ response after CLI, SEI, and PLP").
    assert_eq!(cpu.step(&mut machine), Event::Instruction { opcode: 0x58 });
    assert_eq!(cpu.step(&mut machine), Event::Instruction { opcode: 0xEA });
    machine.log.clear();
    assert_eq!(cpu.step(&mut machine), Event::Interrupt(Interrupt::Irq));
    assert_eq!(machine.log[5], Access::Read(0xFFFE));
    assert_eq!(cpu.state().pc, 0xA000);
}

#[test]
fn rti_restores_i_before_the_poll() {
    let mut machine = Machine::new();
    // Stack holds P with I clear and the return address $8000.
    machine.load(0x8000, &[0xEA]);
    machine.load(0x8100, &[0x40]); // RTI
    let mut cpu = booted(&mut machine, 0x8100);
    let mut state = cpu.state();
    state.s = 0xFA;
    cpu.set_state(state);
    machine.load(0x01FB, &[flags::UNUSED, 0x00, 0x80]);

    machine.lines.irq = true;
    assert_eq!(cpu.step(&mut machine), Event::Instruction { opcode: 0x40 });
    // The IRQ is taken right after RTI, not one instruction later.
    assert_eq!(cpu.step(&mut machine), Event::Interrupt(Interrupt::Irq));
}

#[test]
fn nmi_wins_over_irq() {
    let mut machine = Machine::new();
    machine.load(0x8000, &[0x58, 0xEA, 0xEA]); // CLI, NOP, NOP
    let mut cpu = booted(&mut machine, 0x8000);
    cpu.step(&mut machine); // CLI

    machine.lines.irq = true;
    machine.lines.nmi = true;
    cpu.step(&mut machine);
    assert_eq!(cpu.step(&mut machine), Event::Interrupt(Interrupt::Nmi));
}

#[test]
fn tick_and_step_make_the_same_cycles() {
    let program = [0xA9, 0x10, 0x8D, 0x00, 0x02, 0xEE, 0x00, 0x02, 0xD0, 0xFB];
    let run = |by_tick: bool| {
        let mut machine = Machine::new();
        machine.load(0x8000, &program);
        let mut cpu = booted(&mut machine, 0x8000);
        let mut events = Vec::new();
        while events.len() < 40 {
            if by_tick {
                if let Some(event) = cpu.tick(&mut machine) {
                    events.push(event);
                }
            } else {
                events.push(cpu.step(&mut machine));
            }
        }
        (events, machine.log, cpu.state())
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn state_saved_mid_instruction_resumes_identically() {
    let program = [0xA9, 0x10, 0x8D, 0x00, 0x02, 0xEE, 0x00, 0x02, 0xD0, 0xFB];
    let mut machine = Machine::new();
    machine.load(0x8000, &program);
    let mut cpu = booted(&mut machine, 0x8000);
    // Stop in the middle of an instruction.
    cpu.step(&mut machine);
    cpu.tick(&mut machine);
    cpu.tick(&mut machine);
    let saved = cpu.state();
    assert_ne!(saved.cycle, 0);

    let memory = machine.memory.clone();
    let mut copy = Cpu::new(Model::Rp2a03);
    copy.set_state(saved);
    let mut other = Machine::new();
    other.memory = memory;

    for _ in 0..50 {
        assert_eq!(cpu.tick(&mut machine), copy.tick(&mut other));
    }
    assert_eq!(cpu.state(), copy.state());
}
