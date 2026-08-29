//! key→bytes: translate a key event into the stdin bytes a terminal child expects.

use ratatui::crossterm::event::{KeyCode, KeyModifiers};

/// Translate a key event into the bytes a terminal child expects on stdin, preserving Shift/Ctrl/Alt
/// via xterm's modifier encoding so an agent CLI sees `Shift+↑` etc. instead of a bare arrow.
pub fn key_to_bytes(code: KeyCode, mods: KeyModifiers) -> Option<Vec<u8>> {
    // xterm modifier param: 1 + shift + 2·alt + 4·ctrl; only emitted when a modifier is held.
    let m = 1
        + (mods.contains(KeyModifiers::SHIFT) as u8)
        + ((mods.contains(KeyModifiers::ALT) as u8) << 1)
        + ((mods.contains(KeyModifiers::CONTROL) as u8) << 2);
    // CSI cursor/edit key: bare `ESC[<final>` unmodified, `ESC[1;<m><final>` when modified.
    let csi = |final_: char| {
        if m == 1 {
            format!("\x1b[{final_}").into_bytes()
        } else {
            format!("\x1b[1;{m}{final_}").into_bytes()
        }
    };
    // tilde-form edit key: `ESC[<n>~` bare, `ESC[<n>;<m>~` modified.
    let tilde = |n: u8| {
        if m == 1 {
            format!("\x1b[{n}~").into_bytes()
        } else {
            format!("\x1b[{n};{m}~").into_bytes()
        }
    };
    Some(match code {
        KeyCode::Char(c) => {
            let ctrl = mods.contains(KeyModifiers::CONTROL);
            let alt = mods.contains(KeyModifiers::ALT);
            if ctrl && c.is_ascii_alphabetic() {
                // Ctrl+A..Z -> control byte 0x01..0x1a; Alt adds an ESC prefix.
                let b = (c.to_ascii_uppercase() as u8) & 0x1f;
                if alt { vec![0x1b, b] } else { vec![b] }
            } else if alt {
                let mut v = vec![0x1b];
                v.extend(c.to_string().into_bytes());
                v
            } else {
                c.to_string().into_bytes()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(), // Shift+Tab
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => csi('A'),
        KeyCode::Down => csi('B'),
        KeyCode::Right => csi('C'),
        KeyCode::Left => csi('D'),
        KeyCode::Home => csi('H'),
        KeyCode::End => csi('F'),
        KeyCode::Delete => tilde(3),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_to_bytes_encodes_modifiers() {
        use KeyModifiers as M;
        // Bare specials keep their classic sequences.
        assert_eq!(key_to_bytes(KeyCode::Up, M::NONE).unwrap(), b"\x1b[A");
        assert_eq!(key_to_bytes(KeyCode::Delete, M::NONE).unwrap(), b"\x1b[3~");
        // Modified specials carry the xterm modifier param (shift=2, ctrl=5, alt=3).
        assert_eq!(key_to_bytes(KeyCode::Up, M::SHIFT).unwrap(), b"\x1b[1;2A");
        assert_eq!(key_to_bytes(KeyCode::Up, M::CONTROL).unwrap(), b"\x1b[1;5A");
        assert_eq!(
            key_to_bytes(KeyCode::Delete, M::CONTROL).unwrap(),
            b"\x1b[3;5~"
        );
        // Ctrl+letter is still a control byte; Alt prefixes ESC; Shift+Tab is CSI Z.
        assert_eq!(
            key_to_bytes(KeyCode::Char('c'), M::CONTROL).unwrap(),
            vec![0x03]
        );
        assert_eq!(
            key_to_bytes(KeyCode::Char('a'), M::ALT).unwrap(),
            vec![0x1b, b'a']
        );
        assert_eq!(key_to_bytes(KeyCode::BackTab, M::SHIFT).unwrap(), b"\x1b[Z");
    }
}
