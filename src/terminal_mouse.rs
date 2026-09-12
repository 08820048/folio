//! Mouse events as the child's reporting protocol wants them.
//!
//! When a full-screen program — vim, htop — asks for mouse reports, the
//! wheel and the buttons belong to it, not to the scrollback. What it
//! receives is either the old three-byte form (`ESC [ M` and coordinates
//! as printable bytes, presses only) or SGR's decimal form, which also
//! carries releases and motion. This table is the whole translation; the
//! view decides *whether* an event belongs to the child, this module
//! decides *what to say*.

use crate::terminal_keys::Modifiers;

/// Which button a mouse event carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

/// Encode a button press at a cell position (zero-based column, row).
pub fn press(button: Button, mods: Modifiers, column: usize, row: usize, sgr: bool) -> Vec<u8> {
    match sgr {
        true => sgr_press(code(Some(button), None, mods), column, row, b'M'),
        false => {
            let mut bytes = b"\x1b[M".to_vec();
            bytes.extend(coarse(Some(button), None, mods, column, row));
            bytes
        }
    }
}

/// Encode a button release. The old form does not report releases, so only
/// SGR has anything to say.
pub fn release(
    button: Button,
    mods: Modifiers,
    column: usize,
    row: usize,
    sgr: bool,
) -> Option<Vec<u8>> {
    match sgr {
        // A release keeps the button's own code and swaps the final byte.
        true => Some(sgr_press(code(Some(button), None, mods), column, row, b'm')),
        false => None,
    }
}

/// Encode pointer motion. `button` is the button held while moving, if
/// any; without one only a child asking for hover reports cares.
pub fn motion(
    button: Option<Button>,
    mods: Modifiers,
    column: usize,
    row: usize,
    sgr: bool,
) -> Vec<u8> {
    // Motion carries the motion bit on top of the button, or on top of the
    // code 3 that says no button was down.
    match sgr {
        true => sgr_press(code(button, None, mods) + 32, column, row, b'M'),
        false => {
            let mut bytes = b"\x1b[M".to_vec();
            let mut coarse = coarse(button, None, mods, column, row);
            coarse[0] += 32;
            bytes.extend(coarse);
            bytes
        }
    }
}

/// Encode a wheel turn: up is `true`, down `false`. The wheel never
/// reports a release.
pub fn wheel(up: bool, mods: Modifiers, column: usize, row: usize, sgr: bool) -> Vec<u8> {
    match sgr {
        true => sgr_press(code(None, Some(up), mods), column, row, b'M'),
        false => {
            let mut bytes = b"\x1b[M".to_vec();
            bytes.extend(coarse(None, Some(up), mods, column, row));
            bytes
        }
    }
}

/// The event code: the button, the wheel offset by 64, the motion bit by
/// 32, and the modifiers on top.
fn code(button: Option<Button>, wheel: Option<bool>, mods: Modifiers) -> u8 {
    let mut code = match wheel {
        Some(up) => {
            if up {
                64
            } else {
                65
            }
        }
        None => match button {
            Some(Button::Left) => 0,
            Some(Button::Middle) => 1,
            Some(Button::Right) => 2,
            None => 3,
        },
    };
    if mods.shift {
        code += 4;
    }
    if mods.alt {
        code += 8;
    }
    if mods.ctrl {
        code += 16;
    }
    code
}

/// The old form's three bytes: button and coordinates as printable bytes,
/// with 223 the largest position three bits short of a byte can carry.
fn coarse(
    button: Option<Button>,
    wheel: Option<bool>,
    mods: Modifiers,
    column: usize,
    row: usize,
) -> [u8; 3] {
    let code = code(button, wheel, mods);
    let clamp = |value: usize| 32 + value.min(223) as u8;
    [32 + code, clamp(column + 1), clamp(row + 1)]
}

fn sgr_press(code: u8, column: usize, row: usize, last: u8) -> Vec<u8> {
    format!("\x1b[<{code};{};{}{}", column + 1, row + 1, last as char).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(shift: bool, ctrl: bool, alt: bool) -> Modifiers {
        Modifiers { shift, ctrl, alt }
    }

    #[test]
    fn sgr_speaks_in_decimal_with_releases() {
        assert_eq!(
            press(Button::Left, m(false, false, false), 0, 0, true),
            b"\x1b[<0;1;1M"
        );
        assert_eq!(
            release(Button::Left, m(false, false, false), 0, 0, true),
            Some(b"\x1b[<0;1;1m".to_vec())
        );
        assert_eq!(
            press(Button::Right, m(false, false, false), 9, 4, true),
            b"\x1b[<2;10;5M"
        );
    }

    #[test]
    fn sgr_carries_modifiers_wheels_and_motion() {
        assert_eq!(
            press(Button::Left, m(false, true, false), 0, 0, true),
            b"\x1b[<16;1;1M"
        );
        assert_eq!(
            wheel(true, m(false, false, false), 3, 7, true),
            b"\x1b[<64;4;8M"
        );
        assert_eq!(
            wheel(false, m(true, false, false), 3, 7, true),
            b"\x1b[<69;4;8M"
        );
        // Motion with no button held is the no-button code 3 plus the
        // motion bit; dragging keeps the button's own code plus the bit.
        assert_eq!(
            motion(None, m(false, false, false), 3, 7, true),
            b"\x1b[<35;4;8M"
        );
        assert_eq!(
            motion(Some(Button::Left), m(false, false, false), 3, 7, true),
            b"\x1b[<32;4;8M"
        );
    }

    #[test]
    fn the_old_form_speaks_in_printable_bytes_and_skips_releases() {
        // 32 + the button, then 32 + column + 1 and 32 + row + 1.
        assert_eq!(
            press(Button::Left, m(false, false, false), 10, 5, false),
            vec![27, b'[', b'M', 32, 43, 38]
        );
        assert_eq!(
            release(Button::Left, m(false, false, false), 10, 5, false),
            None
        );
        // Positions past what a byte can hold clamp to the edge.
        assert_eq!(
            press(Button::Left, m(false, false, false), 999, 999, false),
            vec![27, b'[', b'M', 32, 32 + 223, 32 + 223]
        );
    }
}
