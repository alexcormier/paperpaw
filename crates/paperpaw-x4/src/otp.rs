//! What the UC8179's OTP holds, read out for the waveform lab: the
//! controller's own waveforms (UC8179c datasheet, "OTP address mapping",
//! "Temperature range", "LUT format in OTP", pages 47 to 51).
//!
//! The OTP is 6 KB in two banks of 3 KB; a bank starts with the check code
//! `0xA5`, 11 temperature boundaries (TB0 to TB10, in degrees, `0x7F` ends
//! them), the command defaults, the border LUT, and 12 temperature ranges
//! (TR0 to TR11) of 247 bytes each: a header (frame rate, voltages,
//! VCOM_DC), then in black and white mode the LUTs of VCOM, W2W, K2W, W2K and
//! K2K, each of 7 states of 6 bytes (the levels of the four phases, the four
//! frame counts, the repeat). The controller takes the first TR whose
//! boundary is not below the temperature, TR11 above the last.
//!
//! [`describe`] turns bank 0 into lines for the log: what the vendor's own
//! fast waveform is made of, to compare the lab's with.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Bytes of bank 0 that are read: up to the LUT version.
pub const READ_LEN: usize = 0x0BE3;

const CHECK: u8 = 0xA5;
const TB: usize = 0x001;
const DEFAULTS: usize = 0x00C;
const DEFAULTS_END: usize = 0x01F;
const BORDER: usize = 0x01F;
const TR0: usize = 0x049;
const TR_LEN: usize = 0x0F7;
const PRODUCTION: usize = 0x0BDD;
const LUT_VERSION: usize = 0x0BE0;
/// A state: levels, four frame counts, repeat.
const STATE: usize = 6;
const STATES: usize = 7;
const LUT: usize = STATE * STATES;
/// Where the LUTs start in a TR, after its 6 bytes of header.
const HEADER: usize = 6;
const LUTS: [(&str, bool); 5] = [
    ("LUTC", true),
    ("LUTWW", false),
    ("LUTKW", false),
    ("LUTWK", false),
    ("LUTKK", false),
];
const TEMPERATURES: [i32; 2] = [25, 90];

/// The frame rates the controller knows (the `PLL` register's codes).
const RATES: [u16; 16] = [
    5, 10, 15, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 130, 150, 200,
];
/// The gate voltages (`VG_LVL`).
const GATES: [u8; 8] = [9, 10, 11, 12, 17, 18, 19, 20];

/// The voltages the range `celsius` degrees selects runs its waveform at,
/// as the codes of the registers that set them (see
/// [`uc8179::Power`](crate::uc8179::Power)); `None` where bank 0 is not
/// valid or too short.
pub fn power(otp: &[u8], celsius: i32) -> Option<crate::uc8179::Power> {
    if otp.first() != Some(&CHECK) {
        return None;
    }
    let range = tr(otp, select(otp, celsius))?;
    Some(crate::uc8179::Power {
        gate: range[0] & 7,
        vdh: range[1] & 0x3F,
        vdl: range[2] & 0x3F,
        vdhr: range[3] & 0x3F,
        vcom: range[5] & 0x7F,
    })
}

/// The temperature range a temperature of `celsius` degrees selects in
/// bank 0.
pub fn select(otp: &[u8], celsius: i32) -> usize {
    for i in 0..11 {
        let boundary = otp.get(TB + i).copied().unwrap_or(0x7F) as i8;
        if celsius <= i32::from(boundary) {
            return i;
        }
    }
    11
}

fn tr(otp: &[u8], index: usize) -> Option<&[u8]> {
    let start = TR0 + index * TR_LEN;
    otp.get(start..start + TR_LEN)
}

/// A voltage in tenths of a volt as `14.0`.
fn tenths(value: i32) -> String {
    format!("{}.{}", value / 10, value % 10)
}

/// The header of a temperature range, for the log.
fn header(tr: &[u8]) -> String {
    let rate = RATES[usize::from(tr[0] >> 4)];
    let gate = GATES[usize::from(tr[0] & 7)];
    let vdh = 24 + 2 * i32::from(tr[1] & 0x3F);
    let vdl = 24 + 2 * i32::from(tr[2] & 0x3F);
    let vdhr = 24 + 2 * i32::from(tr[3] & 0x3F);
    let vcom = 10 + 5 * i32::from(tr[5] & 0x7F);
    format!(
        "{rate} Hz, VG ±{gate} V, VDH +{} V, VDL -{} V, VDHR +{} V, VCOM_DC -{}.{:02} V",
        tenths(vdh),
        tenths(vdl),
        tenths(vdhr),
        vcom / 100,
        vcom % 100
    )
}

/// One state of a LUT, as `[GND VDL GND GND]x[2 4 0 0]r1`, or nothing for
/// a state that drives for no frame.
fn state(bytes: &[u8], vcom: bool) -> Option<String> {
    let names: [&str; 4] = if vcom {
        ["VCOM", "VDH+VCOM", "VDL+VCOM", "FLOAT"]
    } else {
        ["GND", "VDH", "VDL", "VDHR"]
    };
    let frames = &bytes[1..5];
    if frames.iter().all(|&f| f == 0) {
        return None;
    }
    let phases: Vec<&str> = (0..4)
        .map(|phase| names[usize::from(bytes[0] >> (6 - 2 * phase) & 3)])
        .collect();
    Some(format!(
        "[{}]x[{} {} {} {}]r{}",
        phases.join(" "),
        frames[0],
        frames[1],
        frames[2],
        frames[3],
        bytes[5]
    ))
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::new();
    for byte in bytes {
        text.push_str(&format!("{byte:02X}"));
    }
    text
}

/// What bank 0 of `otp` (its first [`READ_LEN`] bytes) holds, a line each,
/// for the log: the check code, versions, boundaries, defaults, every
/// range's header and, for the ranges 25 and 90 degrees select, their LUTs
/// and bytes.
pub fn describe(otp: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    if otp.len() < READ_LEN {
        lines.push(format!("otp: only {} bytes read", otp.len()));
        return lines;
    }
    lines.push(format!(
        "otp: check {:02X} (A5 is a bank), production {}, LUT version {}",
        otp[0],
        hex(&otp[PRODUCTION..PRODUCTION + 3]),
        hex(&otp[LUT_VERSION..LUT_VERSION + 3])
    ));
    if otp[0] != CHECK {
        lines.push(format!(
            "otp: first bytes {} (bank 0 is not valid, bank 1 not read)",
            hex(&otp[..16])
        ));
        return lines;
    }
    let boundaries: Vec<String> = otp[TB..TB + 11]
        .iter()
        .map(|&b| format!("{}", b as i8))
        .collect();
    lines.push(format!(
        "otp: temperature boundaries {} (127 ends them)",
        boundaries.join(" ")
    ));
    lines.push(format!(
        "otp: command defaults {}",
        hex(&otp[DEFAULTS..DEFAULTS_END])
    ));
    lines.push(format!("otp: border LUT {}", hex(&otp[BORDER..TR0])));
    for index in 0..12 {
        if let Some(range) = tr(otp, index) {
            lines.push(format!("otp: TR{index}: {}", header(range)));
        }
    }
    // The ranges the boundaries can select, to the one that ends them.
    let last = (0..11).find(|&i| otp[TB + i] == 0x7F).unwrap_or(11);
    let mut shown = Vec::new();
    for celsius in TEMPERATURES {
        let index = select(otp, celsius);
        if shown.contains(&index) {
            lines.push(format!("otp: at {celsius} C: TR{index}, as above"));
            continue;
        }
        shown.push(index);
        lines.push(format!("otp: at {celsius} C: TR{index}"));
        let Some(range) = tr(otp, index) else {
            continue;
        };
        for (n, (name, vcom)) in LUTS.into_iter().enumerate() {
            let start = HEADER + n * LUT;
            for s in 0..STATES {
                let bytes = &range[start + s * STATE..start + (s + 1) * STATE];
                if let Some(text) = state(bytes, vcom) {
                    lines.push(format!("otp: TR{index} {name} s{s} {text}"));
                }
            }
        }
        for (row, chunk) in range.chunks(48).enumerate() {
            lines.push(format!(
                "otp: TR{index} raw {:03X}: {}",
                row * 48,
                hex(chunk)
            ));
        }
    }
    // The other ranges that can be selected: the states of what turns a
    // pixel (K2W and W2K), to see how they differ.
    for index in (0..=last).filter(|i| !shown.contains(i)) {
        let Some(range) = tr(otp, index) else {
            continue;
        };
        for (n, (name, _)) in LUTS
            .into_iter()
            .enumerate()
            .filter(|(n, _)| (2..4).contains(n))
        {
            let start = HEADER + n * LUT;
            for s in 0..STATES {
                let bytes = &range[start + s * STATE..start + (s + 1) * STATE];
                if let Some(text) = state(bytes, false) {
                    lines.push(format!("otp: TR{index} {name} s{s} {text}"));
                }
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// An OTP of bank 0: boundaries 0, 10, 30 °C and 7F; TR2 (up to 30 °C)
    /// and TR3 as the last, with a K2W state of one phase.
    fn otp() -> Vec<u8> {
        let mut otp = vec![0xFF; READ_LEN];
        otp[0] = CHECK;
        otp[TB..TB + 4].copy_from_slice(&[0x00, 0x0A, 0x1E, 0x7F]);
        // TR2: 50 Hz (code 6), VG 20 V (7); VDH 15 V (63), VDL 14 V (58),
        // VCOM_DC -2.00 V (38); no LUT drives but one state.
        let two = TR0 + 2 * TR_LEN;
        otp[two..two + HEADER].copy_from_slice(&[0x67, 63, 58, 0, 0, 38]);
        for byte in &mut otp[two + HEADER..two + HEADER + 5 * LUT] {
            *byte = 0;
        }
        // Its LUTKW (the third LUT) state 1: phases GND, VDL, GND, GND,
        // frames 0, 6, 0, 0, repeat 1.
        let kw = two + HEADER + 2 * LUT + STATE;
        otp[kw..kw + STATE].copy_from_slice(&[0b00_10_00_00, 0, 6, 0, 0, 1]);
        otp
    }

    #[test]
    fn the_temperature_picks_the_range_by_the_boundaries() {
        let otp = otp();
        assert_eq!(select(&otp, -40), 0);
        assert_eq!(select(&otp, 0), 0);
        assert_eq!(select(&otp, 5), 1);
        assert_eq!(select(&otp, 25), 2);
        assert_eq!(select(&otp, 30), 2);
        // Above the 30 the boundary 127 ends the search: TR3.
        assert_eq!(select(&otp, 90), 3);
    }

    #[test]
    fn a_header_is_told_in_volts_and_hertz() {
        let otp = otp();
        let text = header(tr(&otp, 2).unwrap());
        assert_eq!(
            text,
            "50 Hz, VG ±20 V, VDH +15.0 V, VDL -14.0 V, VDHR +2.4 V, VCOM_DC -2.00 V"
        );
    }

    #[test]
    fn the_log_has_the_selected_range_and_its_driving_states() {
        let lines = describe(&otp());
        let all = lines.join("\n");
        assert!(all.contains("otp: check A5"), "{all}");
        assert!(all.contains("temperature boundaries 0 10 30 127"), "{all}");
        assert!(all.contains("otp: at 25 C: TR2"), "{all}");
        assert!(
            all.contains("otp: TR2 LUTKW s1 [GND VDL GND GND]x[0 6 0 0]r1"),
            "{all}"
        );
        // Only states that drive are told.
        assert_eq!(all.matches("TR2 LUTKW s").count(), 1, "{all}");
        assert!(all.contains("otp: TR2 raw 000: 67"), "{all}");
    }

    #[test]
    fn the_voltages_of_the_range_are_given_as_register_codes() {
        let power = power(&otp(), 25).unwrap();
        assert_eq!(
            (power.gate, power.vdh, power.vdl, power.vcom),
            (7, 63, 58, 38)
        );
        assert!(super::power(&[0; 10], 25).is_none());
    }

    #[test]
    fn the_other_ranges_that_can_be_selected_are_told_too() {
        let mut otp = otp();
        // TR3 turns a pixel one way for 4 frames.
        let three = TR0 + 3 * TR_LEN;
        for byte in &mut otp[three + HEADER..three + HEADER + 5 * LUT] {
            *byte = 0;
        }
        let wk = three + HEADER + 3 * LUT;
        otp[wk..wk + STATE].copy_from_slice(&[0b01_01_01_01, 4, 0, 0, 0, 0]);
        let all = describe(&otp).join("\n");
        assert!(
            all.contains("otp: TR3 LUTWK s0 [VDH VDH VDH VDH]x[4 0 0 0]r0"),
            "{all}"
        );
    }

    #[test]
    fn an_empty_or_short_read_says_so_and_stops() {
        assert_eq!(describe(&[0; 10]), ["otp: only 10 bytes read"]);
        let mut blank = vec![0xFF; READ_LEN];
        blank[0] = 0x00;
        let lines = describe(&blank);
        assert!(lines.last().unwrap().contains("bank 0 is not valid"));
    }
}
