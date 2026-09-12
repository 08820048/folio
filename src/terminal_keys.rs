//! Turning key semantics into the bytes a terminal program expects.
//!
//! The editor's keys are names; a pty wants the byte sequences xterm's
//! manual calls "PC-Style Function Keys" and the DEC manuals call
//! everything else. This table is the whole translation, kept free of GPUI
//! so the UI hands over a key and a modifier set and writes whatever comes
//! back. `None` means the key has no terminal meaning and stays the
//! editor's.

/// The keyboard modifiers held while a key went down. The command key is
/// not among them: it belongs to the application's bindings, not the
/// child's input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Modifiers {
    /// The xterm modifier parameter: 1 plus the bits, present only once a
    /// modifier is actually held.
    fn parameter(self) -> Option<u8> {
        let bits = self.shift as u8 + 2 * self.alt as u8 + 4 * self.ctrl as u8;
        (bits != 0).then_some(1 + bits)
    }
}

/// The keys a terminal answers to, named rather than encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    F(u8),
}

/// Encode a key for the pty. `app_cursor` is the terminal's DECCKM state:
/// with it on, an unmodified arrow or Home/End answers with SS3 instead of
/// CSI. A modified cursor key always takes the CSI form, which carries the
/// modifier parameter.
pub fn encode(key: Key, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    match key {
        Key::Char(c) => char_bytes(c, mods),
        Key::Enter => escape_prefix(b"\r", mods),
        Key::Tab => {
            if mods.shift {
                Some(b"\x1b[Z".to_vec())
            } else {
                escape_prefix(b"\t", mods)
            }
        }
        Key::Backspace => {
            if mods.ctrl {
                escape_prefix(b"\x08", mods)
            } else {
                escape_prefix(b"\x7f", mods)
            }
        }
        Key::Escape => Some(b"\x1b".to_vec()),
        Key::Insert => csi_tilde(2, mods),
        Key::Delete => csi_tilde(3, mods),
        Key::PageUp => csi_tilde(5, mods),
        Key::PageDown => csi_tilde(6, mods),
        Key::Home => cursor_key('H', mods, app_cursor),
        Key::End => cursor_key('F', mods, app_cursor),
        Key::Up => cursor_key('A', mods, app_cursor),
        Key::Down => cursor_key('B', mods, app_cursor),
        Key::Right => cursor_key('C', mods, app_cursor),
        Key::Left => cursor_key('D', mods, app_cursor),
        Key::F(n) => function_key(n, mods),
    }
}

/// Text pasted into the terminal. With bracketed paste on — the mode lives
/// in the grid, see `Terminal::paste` — the text rides inside the bracket
/// pair verbatim, its own end markers stripped out; without it, line breaks
/// become the carriage returns an enter would have typed.
pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let mut out = b"\x1b[200~".to_vec();
        for part in text.split("\x1b[201~") {
            out.extend_from_slice(part.as_bytes());
        }
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.replace("\r\n", "\n").replace('\n', "\r").into_bytes()
    }
}

/// A character's bytes: its UTF-8 as typed, a control byte under ctrl, and
/// an escape in front of either under alt. A ctrl over something with no
/// control byte — a non-ASCII letter, say — is `None`.
fn char_bytes(c: char, mods: Modifiers) -> Option<Vec<u8>> {
    let mut bytes = c.to_string().into_bytes();
    if mods.ctrl {
        let lower = c.to_ascii_lowercase();
        bytes = vec![match lower {
            ' ' | '@' => 0x00,
            '[' => 0x1b,
            '\\' => 0x1c,
            ']' => 0x1d,
            '^' => 0x1e,
            '_' => 0x1f,
            letter @ 'a'..='z' => letter as u8 - b'a' + 1,
            _ => return None,
        }];
    }
    if mods.alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

fn escape_prefix(bytes: &[u8], mods: Modifiers) -> Option<Vec<u8>> {
    let mut bytes = bytes.to_vec();
    if mods.alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

/// CSI `{code}~`, carrying the modifier parameter when one is held.
fn csi_tilde(code: u8, mods: Modifiers) -> Option<Vec<u8>> {
    let tail = match mods.parameter() {
        Some(parameter) => format!(";{parameter}"),
        None => String::new(),
    };
    Some(format!("\x1b[{code}{tail}~").into_bytes())
}

/// Arrows and Home/End: SS3 while the cursor mode is on, CSI otherwise, and
/// the parametered CSI form the moment a modifier joins.
fn cursor_key(letter: char, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    match mods.parameter() {
        Some(parameter) => Some(format!("\x1b[1;{parameter}{letter}").into_bytes()),
        None if app_cursor => Some(format!("\x1bO{letter}").into_bytes()),
        None => Some(format!("\x1b[{letter}").into_bytes()),
    }
}

/// F1–F4 answer with SS3 letters, F5–F12 with the numbered tilde forms;
/// modifiers switch the first four over to the parametered CSI form too.
fn function_key(n: u8, mods: Modifiers) -> Option<Vec<u8>> {
    match n {
        1..=4 => {
            let letter = match n {
                1 => 'P',
                2 => 'Q',
                3 => 'R',
                _ => 'S',
            };
            match mods.parameter() {
                Some(parameter) => Some(format!("\x1b[1;{parameter}{letter}").into_bytes()),
                None => Some(format!("\x1bO{letter}").into_bytes()),
            }
        }
        5..=12 => {
            let code = match n {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                _ => 24,
            };
            csi_tilde(code, mods)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(shift: bool, ctrl: bool, alt: bool) -> Modifiers {
        Modifiers { shift, ctrl, alt }
    }

    fn encoded(key: Key, mods: Modifiers, app_cursor: bool) -> String {
        String::from_utf8(encode(key, mods, app_cursor).unwrap()).unwrap()
    }

    #[test]
    fn plain_characters_are_their_utf8_bytes() {
        assert_eq!(encoded(Key::Char('a'), m(false, false, false), false), "a");
        assert_eq!(
            encoded(Key::Char('中'), m(false, false, false), false),
            "中"
        );
    }

    #[test]
    fn ctrl_compounds_to_control_bytes() {
        assert_eq!(
            encoded(Key::Char('c'), m(false, true, false), false),
            "\x03"
        );
        assert_eq!(
            encoded(Key::Char(' '), m(false, true, false), false),
            "\x00"
        );
        // The two that collide with editing keys land on the control bytes.
        assert_eq!(encoded(Key::Char('i'), m(false, true, false), false), "\t");
        assert_eq!(encoded(Key::Char('m'), m(false, true, false), false), "\r");
        // And a ctrl with nothing to control is not the terminal's to take.
        assert_eq!(encode(Key::Char('中'), m(false, true, false), false), None);
    }

    #[test]
    fn alt_prefixes_an_escape() {
        assert_eq!(
            encoded(Key::Char('a'), m(false, false, true), false),
            "\x1ba"
        );
        assert_eq!(encoded(Key::Enter, m(false, false, true), false), "\x1b\r");
        assert_eq!(
            encoded(Key::Char('c'), m(false, true, true), false),
            "\x1b\x03"
        );
    }

    #[test]
    fn arrows_follow_the_cursor_mode_and_carry_modifiers() {
        assert_eq!(encoded(Key::Up, m(false, false, false), false), "\x1b[A");
        assert_eq!(encoded(Key::Up, m(false, false, false), true), "\x1bOA");
        assert_eq!(encoded(Key::Down, m(false, false, false), true), "\x1bOB");
        assert_eq!(encoded(Key::Right, m(false, false, false), false), "\x1b[C");
        assert_eq!(encoded(Key::Left, m(false, false, false), false), "\x1b[D");
        assert_eq!(encoded(Key::Up, m(false, true, false), false), "\x1b[1;5A");
        assert_eq!(encoded(Key::Left, m(true, true, false), false), "\x1b[1;6D");
        assert_eq!(encoded(Key::Right, m(true, false, true), true), "\x1b[1;4C");
    }

    #[test]
    fn home_end_and_editing_keys() {
        assert_eq!(encoded(Key::Home, m(false, false, false), false), "\x1b[H");
        assert_eq!(encoded(Key::Home, m(false, false, false), true), "\x1bOH");
        assert_eq!(encoded(Key::End, m(false, false, false), false), "\x1b[F");
        assert_eq!(encoded(Key::End, m(true, false, false), false), "\x1b[1;2F");
        assert_eq!(
            encoded(Key::Delete, m(false, false, false), false),
            "\x1b[3~"
        );
        assert_eq!(
            encoded(Key::Delete, m(false, true, false), false),
            "\x1b[3;5~"
        );
        assert_eq!(
            encoded(Key::Insert, m(false, false, false), false),
            "\x1b[2~"
        );
        assert_eq!(
            encoded(Key::PageUp, m(false, false, false), false),
            "\x1b[5~"
        );
        assert_eq!(
            encoded(Key::PageDown, m(false, false, false), false),
            "\x1b[6~"
        );
        assert_eq!(
            encoded(Key::Backspace, m(false, false, false), false),
            "\x7f"
        );
        assert_eq!(
            encoded(Key::Backspace, m(false, true, false), false),
            "\x08"
        );
        assert_eq!(encoded(Key::Escape, m(false, false, false), false), "\x1b");
        assert_eq!(encoded(Key::Tab, m(false, false, false), false), "\t");
        assert_eq!(encoded(Key::Tab, m(true, false, false), false), "\x1b[Z");
    }

    #[test]
    fn function_keys() {
        assert_eq!(encoded(Key::F(1), m(false, false, false), false), "\x1bOP");
        assert_eq!(encoded(Key::F(4), m(false, false, false), false), "\x1bOS");
        assert_eq!(
            encoded(Key::F(5), m(false, false, false), false),
            "\x1b[15~"
        );
        assert_eq!(
            encoded(Key::F(10), m(false, false, false), false),
            "\x1b[21~"
        );
        assert_eq!(
            encoded(Key::F(12), m(false, false, false), false),
            "\x1b[24~"
        );
        assert_eq!(
            encoded(Key::F(5), m(false, false, true), false),
            "\x1b[15;3~"
        );
        assert_eq!(encode(Key::F(13), m(false, false, false), false), None);
    }

    #[test]
    fn paste_wraps_when_bracketed_and_compacts_newlines_when_not() {
        let bracketed = String::from_utf8(paste("a\nb", true)).unwrap();
        assert_eq!(bracketed, "\x1b[200~a\nb\x1b[201~");

        // An end marker inside the text cannot cut the bracket short.
        let hostile = String::from_utf8(paste("a\x1b[201~b", true)).unwrap();
        assert_eq!(hostile, "\x1b[200~ab\x1b[201~");

        // CRLF and LF both become one return; a lone return stays one.
        assert_eq!(paste("a\r\nb\nc\rd", false), b"a\rb\rc\rd");
    }
}
