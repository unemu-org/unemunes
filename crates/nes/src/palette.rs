//! The RGB colours of the PPU's 64 colours under its three emphasis bits,
//! computed from the composite signal the PPU draws (design note §14 Q6).
//!
//! Source: NESdev wiki, "NTSC video", revision 24244, sections "Brightness
//! Levels", "Color Phases", "Color Tint Bits" and "Composite decoding". The
//! PPU has no RGB palette; it draws each colour `$xy` as a square wave over
//! the 12 phases of the colour subcarrier, between a low and a high voltage
//! chosen by the brightness `x`:
//!
//! - `$x0` stays at the high level, `$xD` at the low level, `$xE` and `$xF`
//!   at the level of `$1D` (black), and `$x1`–`$xC` alternate, high on the 6
//!   phases of wave `y`;
//! - an emphasis bit attenuates the signal on the 6 phases of its own wave
//!   (bit 5: wave `$C`, bit 6: wave 4, bit 7: wave 8), except for `$xE` and
//!   `$xF`; attenuated levels are measured values too.
//!
//! The PPU's output impedance depends on the signal level, which delays the
//! chroma of brighter colours: "Differential Phase Distortion" estimates
//! about -5° of hue rotation per palette row on a 2C02G (-2.5° on a 2C02E),
//! from hardware captures. The table models the 2C02G: the decoded chroma of
//! row `r` is rotated by `-5° × r`, so row 0 keeps its hue.
//!
//! The voltages are the terminated measurements the page lists. Decoding
//! takes the mean over a subcarrier cycle as luma and demodulates U and V
//! against the colour burst, which has the phase of wave 8 (the `-U` axis),
//! with the factor of 2 the page gives; YUV becomes RGB with the page's
//! inverse matrix, and the range from 7.5 IRE (US set-up) to 100 IRE maps to
//! 0–255 with clipping. The demodulation angles are multiples of 30°, so the
//! code uses their exact sines and cosines and no transcendental function:
//! the table is the same on every host.

/// Measured voltages in millivolts: low (`$xD`) and high (`$x0`) levels per
/// brightness, plain and attenuated.
const LOW: [f64; 4] = [228.0, 312.0, 552.0, 880.0];
const HIGH: [f64; 4] = [616.0, 840.0, 1100.0, 1100.0];
const LOW_ATTENUATED: [f64; 4] = [192.0, 256.0, 448.0, 712.0];
const HIGH_ATTENUATED: [f64; 4] = [500.0, 676.0, 896.0, 896.0];
/// `$1D`, the blanking level: 0 IRE.
const BLANK: f64 = 312.0;
/// "Normalizing signals" scales by 140 IRE to the volt.
const MV_PER_IRE: f64 = 1000.0 / 140.0;

/// cos(k × 30°) for k = 0..12, exactly.
const COS30: [f64; 12] = {
    const H: f64 = 0.866_025_403_784_438_6; // √3 / 2
    [1.0, H, 0.5, 0.0, -0.5, -H, -1.0, -H, -0.5, 0.0, 0.5, H]
};

/// cos and sin of the 2C02G's differential phase rotation, -5° × row, for
/// rows 0 to 3 (exact decimal values of cos and sin of 0°, 5°, 10°, 15°).
const ROW_COS: [f64; 4] = [
    1.0,
    0.996_194_698_091_745_5,
    0.984_807_753_012_208,
    0.965_925_826_289_068_3,
];
const ROW_SIN: [f64; 4] = [
    0.0,
    -0.087_155_742_747_658_17,
    -0.173_648_177_666_930_33,
    -0.258_819_045_102_520_74,
];

/// Whether wave `n` (1 to 12) is high in phase `p` (0 to 11), from the
/// page's phase diagram: wave 1 is high in phases 0–5, and each next wave one
/// phase earlier.
fn wave_high(n: usize, p: usize) -> bool {
    (p + n - 1) % 12 < 6
}

fn ire(mv: f64) -> f64 {
    (mv - BLANK) / MV_PER_IRE
}

/// The signal of colour `index` (6 bits) under `emphasis` (PPUMASK bits 5–7
/// as 0–7) in each of the 12 phases, in IRE.
fn signal(index: u8, emphasis: u8) -> [f64; 12] {
    let brightness = usize::from(index >> 4 & 3);
    let hue = usize::from(index & 0x0F);
    let mut out = [0.0; 12];
    for (p, sample) in out.iter_mut().enumerate() {
        if hue >= 0x0E {
            *sample = ire(BLANK);
            continue;
        }
        let high = match hue {
            0x00 => true,
            0x0D => false,
            n => wave_high(n, p),
        };
        let attenuated = (emphasis & 1 != 0 && wave_high(12, p))
            || (emphasis & 2 != 0 && wave_high(4, p))
            || (emphasis & 4 != 0 && wave_high(8, p));
        let mv = match (high, attenuated) {
            (true, false) => HIGH[brightness],
            (false, false) => LOW[brightness],
            (true, true) => HIGH_ATTENUATED[brightness],
            (false, true) => LOW_ATTENUATED[brightness],
        };
        *sample = ire(mv);
    }
    out
}

/// `0x00RRGGBB` for colour `index` under `emphasis` (0–7).
pub fn rgb(index: u8, emphasis: u8) -> u32 {
    let s = signal(index & 0x3F, emphasis & 7);
    // Wave 2 (blue, the +U axis) is centred on phase 1.5, so the reference
    // angle of phase p is θ = 45° - p × 30°: wave 6 (red) lands at +120° and
    // wave 10 (green) at -120°. With a = p × 30°,
    //   cos θ = (cos a + sin a) × √2/2,  sin θ = (cos a - sin a) × √2/2.
    let mut y = 0.0;
    let mut cos_sum = 0.0;
    let mut sin_sum = 0.0;
    for (p, &sample) in s.iter().enumerate() {
        y += sample;
        cos_sum += sample * COS30[p];
        sin_sum += sample * COS30[(p + 9) % 12]; // sin a = cos(a - 90°)
    }
    const R: f64 = core::f64::consts::FRAC_1_SQRT_2;
    y /= 12.0;
    // The factor 2 corrects the demodulator's gain ("Chroma saturation
    // correction").
    let u = 2.0 * (cos_sum + sin_sum) * R / 12.0;
    let v = 2.0 * (cos_sum - sin_sum) * R / 12.0;
    // Differential phase distortion: rotate by -5° per row.
    let row = usize::from(index >> 4 & 3);
    let (c, s) = (ROW_COS[row], ROW_SIN[row]);
    let (u, v) = (u * c - v * s, u * s + v * c);

    let r = y + v * 1.139_883;
    let g = y - u * 0.394_642 - v * 0.580_622;
    let b = y + u * 2.032_062;
    let channel = |c: f64| {
        let n = ((c - 7.5) / (100.0 - 7.5)).clamp(0.0, 1.0);
        // "Signal RGB into sRGB": (int)(C × 255).
        (n * 255.0) as u32
    };
    channel(r) << 16 | channel(g) << 8 | channel(b)
}

/// All 512 colours: index `emphasis << 6 | colour`.
pub fn table() -> Vec<u32> {
    (0..512u16)
        .map(|i| rgb((i & 0x3F) as u8, (i >> 6) as u8))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channels(c: u32) -> (u32, u32, u32) {
        (c >> 16, c >> 8 & 0xFF, c & 0xFF)
    }

    #[test]
    fn blacks_greys_and_whites() {
        assert_eq!(rgb(0x0F, 0), 0, "$xF is the blanking level, below set-up");
        assert_eq!(rgb(0x1D, 0), 0);
        assert_eq!(rgb(0x20, 0), 0xFF_FF_FF, "110 IRE clips to white");
        assert_eq!(rgb(0x30, 0), rgb(0x20, 0), "$20 and $30 are the same");
        let (r, g, b) = channels(rgb(0x00, 0));
        assert!(
            r == g && g == b && (90..110).contains(&r),
            "grey: {r} {g} {b}"
        );
    }

    #[test]
    fn hues_follow_the_tint_bit_complements() {
        // "Color Tint Bits": colour 2 is blue, 6 red, A green.
        let (r, g, b) = channels(rgb(0x12, 0));
        assert!(b > r && b > g, "$12 blue: {r} {g} {b}");
        let (r, g, b) = channels(rgb(0x16, 0));
        assert!(r > g && r > b, "$16 red: {r} {g} {b}");
        let (r, g, b) = channels(rgb(0x1A, 0));
        assert!(g > r && g > b, "$1A green: {r} {g} {b}");
    }

    #[test]
    fn brighter_rows_turn_their_hue_by_the_2c02g_phase_distortion() {
        // Row 0 is not turned; row 2 by -10°, which takes $22 from violet to
        // blue: blue well above red, red no longer near blue.
        let (r, g, b) = channels(rgb(0x22, 0));
        assert!(b == 255 && r < 0xA0 && g < 0xA0, "$22: {r} {g} {b}");
        assert_eq!(rgb(0x22, 0), 0x96_8A_FF, "pinned for the 2C02G model");
        let (r1, _, b1) = channels(rgb(0x21, 0));
        assert!(b1 > r1 + 150, "$21 sky blue: {r1} {b1}");
    }

    #[test]
    fn emphasis_darkens_all_but_columns_e_and_f() {
        let plain = channels(rgb(0x20, 0));
        let all = channels(rgb(0x20, 7));
        assert!(all.0 < plain.0 && all.1 < plain.1 && all.2 < plain.2);
        assert_eq!(rgb(0x0F, 7), rgb(0x0F, 0));
        // Emphasising red (bit 5) keeps red stronger than green.
        let (r, g, _) = channels(rgb(0x30, 1));
        assert!(r > g, "red emphasis: {r} {g}");
    }

    #[test]
    fn the_table_has_512_entries_indexed_by_emphasis_and_colour() {
        let t = table();
        assert_eq!(t.len(), 512);
        assert_eq!(t[0x16 | 2 << 6], rgb(0x16, 2));
    }
}
