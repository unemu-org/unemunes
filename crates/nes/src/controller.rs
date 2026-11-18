//! The standard controller (NESdev wiki, "Standard controller", revision
//! 23466).
//!
//! A write to `$4016` sets the strobe from bit 0. While the strobe is high the
//! controller keeps reloading its shift register from the buttons, so every
//! read reports A; when it goes low, the reads report A, B, Select, Start, Up,
//! Down, Left, Right through bit 0, then 1 for every read after the eighth on
//! an official controller.

/// Buttons held, one bit each in report order: bit 0 is A, bit 7 Right.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Buttons(pub u8);

impl Buttons {
    pub const A: Buttons = Buttons(0x01);
    pub const B: Buttons = Buttons(0x02);
    pub const SELECT: Buttons = Buttons(0x04);
    pub const START: Buttons = Buttons(0x08);
    pub const UP: Buttons = Buttons(0x10);
    pub const DOWN: Buttons = Buttons(0x20);
    pub const LEFT: Buttons = Buttons(0x40);
    pub const RIGHT: Buttons = Buttons(0x80);

    /// These buttons and `other`.
    pub const fn with(self, other: Buttons) -> Buttons {
        Buttons(self.0 | other.0)
    }
}

/// One controller port with a standard controller in it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Controller {
    /// What the player holds now.
    pub buttons: Buttons,
    shift: u8,
    reads: u8,
    strobe: bool,
}

impl Controller {
    /// Bit 0 of a `$4016` write. The shift register reloads all the time the
    /// strobe is high, so the write that takes it low latches the buttons
    /// held at that moment.
    pub fn write_strobe(&mut self, high: bool) {
        if high || self.strobe {
            self.reload();
        }
        self.strobe = high;
    }

    /// The bit a read of the port reports, then the shift.
    pub fn read(&mut self) -> u8 {
        if self.strobe {
            self.reload();
            return self.buttons.0 & 1;
        }
        if self.reads >= 8 {
            return 1;
        }
        let bit = self.shift & 1;
        self.shift >>= 1;
        self.reads += 1;
        bit
    }

    fn reload(&mut self) {
        self.shift = self.buttons.0;
        self.reads = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latched(buttons: Buttons) -> Controller {
        let mut pad = Controller {
            buttons,
            ..Controller::default()
        };
        pad.write_strobe(true);
        pad.write_strobe(false);
        pad
    }

    #[test]
    fn reports_a_b_select_start_up_down_left_right() {
        let mut pad = latched(Buttons::B.with(Buttons::START).with(Buttons::RIGHT));
        let bits: Vec<u8> = (0..8).map(|_| pad.read()).collect();
        assert_eq!(bits, [0, 1, 0, 1, 0, 0, 0, 1]);
    }

    #[test]
    fn reads_after_the_eighth_report_1() {
        let mut pad = latched(Buttons::default());
        for _ in 0..8 {
            assert_eq!(pad.read(), 0);
        }
        assert_eq!(pad.read(), 1);
        assert_eq!(pad.read(), 1);
    }

    #[test]
    fn a_high_strobe_reports_a_on_every_read() {
        let mut pad = Controller {
            buttons: Buttons::A,
            ..Controller::default()
        };
        pad.write_strobe(true);
        for _ in 0..10 {
            assert_eq!(pad.read(), 1);
        }
    }

    #[test]
    fn buttons_pressed_while_the_strobe_is_high_are_latched_when_it_falls() {
        let mut pad = Controller::default();
        pad.write_strobe(true);
        pad.buttons = Buttons::A;
        pad.write_strobe(false);
        assert_eq!(pad.read(), 1);
    }

    #[test]
    fn the_report_is_latched_when_the_strobe_falls() {
        let mut pad = latched(Buttons::A);
        pad.buttons = Buttons::default();
        assert_eq!(pad.read(), 1, "A was held at the latch");
    }
}
