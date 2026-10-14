//! Hotkey strings such as `"ctrl+shift+esc"` <-> Windows virtual-key codes.

pub const VK_SHIFT: u16 = 0x10;
pub const VK_CONTROL: u16 = 0x11;
pub const VK_MENU: u16 = 0x12;
pub const VK_LWIN: u16 = 0x5B;

/// Named keys. Letters and digits are handled separately.
const NAMED: &[(&str, u16)] = &[
    ("backspace", 0x08),
    ("tab", 0x09),
    ("enter", 0x0D),
    ("pause", 0x13),
    ("capslock", 0x14),
    ("esc", 0x1B),
    ("space", 0x20),
    ("pageup", 0x21),
    ("pagedown", 0x22),
    ("end", 0x23),
    ("home", 0x24),
    ("left", 0x25),
    ("up", 0x26),
    ("right", 0x27),
    ("down", 0x28),
    ("printscreen", 0x2C),
    ("insert", 0x2D),
    ("delete", 0x2E),
    ("apps", 0x5D),
    ("num0", 0x60),
    ("num1", 0x61),
    ("num2", 0x62),
    ("num3", 0x63),
    ("num4", 0x64),
    ("num5", 0x65),
    ("num6", 0x66),
    ("num7", 0x67),
    ("num8", 0x68),
    ("num9", 0x69),
    ("nummultiply", 0x6A),
    ("numplus", 0x6B),
    ("numminus", 0x6D),
    ("numdecimal", 0x6E),
    ("numdivide", 0x6F),
    ("numlock", 0x90),
    ("scrolllock", 0x91),
    ("volumemute", 0xAD),
    ("volumedown", 0xAE),
    ("volumeup", 0xAF),
    ("medianext", 0xB0),
    ("mediaprev", 0xB1),
    ("mediastop", 0xB2),
    ("mediaplaypause", 0xB3),
    ("semicolon", 0xBA),
    ("plus", 0xBB),
    ("comma", 0xBC),
    ("minus", 0xBD),
    ("period", 0xBE),
    ("slash", 0xBF),
    ("backquote", 0xC0),
    ("lbracket", 0xDB),
    ("backslash", 0xDC),
    ("rbracket", 0xDD),
    ("quote", 0xDE),
];

const ALIASES: &[(&str, &str)] = &[
    ("escape", "esc"),
    ("return", "enter"),
    ("del", "delete"),
    ("ins", "insert"),
    ("pgup", "pageup"),
    ("pgdn", "pagedown"),
    ("prtsc", "printscreen"),
    (".", "period"),
    (",", "comma"),
    ("-", "minus"),
];

/// A parsed key combination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotkey {
    /// Modifier virtual keys, in press order.
    pub modifiers: Vec<u16>,
    pub key: u16,
}

pub fn vk_from_name(name: &str) -> Option<u16> {
    let lower = name.to_ascii_lowercase();
    let n = ALIASES
        .iter()
        .find(|(a, _)| *a == lower)
        .map_or(lower.as_str(), |(_, t)| t);
    if n.len() == 1 {
        let c = n.as_bytes()[0];
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase() as u16);
        }
    }
    if let Some(f) = n.strip_prefix('f').and_then(|d| d.parse::<u16>().ok()) {
        if (1..=24).contains(&f) {
            return Some(0x6F + f);
        }
    }
    if let Some(hex) = n.strip_prefix("vk:0x") {
        return u16::from_str_radix(hex, 16).ok();
    }
    NAMED.iter().find(|(k, _)| *k == n).map(|&(_, vk)| vk)
}

pub fn name_from_vk(vk: u16) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => ((vk as u8) as char).to_ascii_lowercase().to_string(),
        0x70..=0x87 => format!("f{}", vk - 0x6F),
        _ => NAMED
            .iter()
            .find(|&&(_, v)| v == vk)
            .map(|(n, _)| n.to_string())
            .unwrap_or_else(|| format!("vk:0x{vk:02x}")),
    }
}

pub fn parse(s: &str) -> Result<Hotkey, String> {
    let mut modifiers = Vec::new();
    let mut key = None;
    // Split on '+', but let "numplus"/"plus" be the way to write a literal plus.
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        let m = match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Some(VK_CONTROL),
            "shift" => Some(VK_SHIFT),
            "alt" => Some(VK_MENU),
            "win" | "meta" | "super" => Some(VK_LWIN),
            _ => None,
        };
        match (m, key) {
            (Some(vk), _) => modifiers.push(vk),
            (None, None) => {
                key = Some(
                    vk_from_name(part).ok_or_else(|| format!("unknown key {part:?} in {s:?}"))?,
                )
            }
            (None, Some(_)) => return Err(format!("{s:?} has more than one non-modifier key")),
        }
    }
    // A bare modifier ("win") is a valid hotkey on its own.
    let key = match key {
        Some(k) => k,
        None => modifiers
            .pop()
            .ok_or_else(|| format!("empty hotkey {s:?}"))?,
    };
    Ok(Hotkey { modifiers, key })
}

/// Formats a combination the way [`parse`] reads it.
pub fn format(ctrl: bool, shift: bool, alt: bool, win: bool, vk: u16) -> String {
    let mut parts = Vec::new();
    for (on, name) in [(ctrl, "ctrl"), (shift, "shift"), (alt, "alt"), (win, "win")] {
        if on {
            parts.push(name.to_string());
        }
    }
    parts.push(name_from_vk(vk));
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combinations() {
        let h = parse("Ctrl+Shift+Esc").unwrap();
        assert_eq!(h.modifiers, vec![VK_CONTROL, VK_SHIFT]);
        assert_eq!(h.key, 0x1B);
        assert_eq!(parse("win+numminus").unwrap().key, 0x6D);
        assert_eq!(parse("win+.").unwrap().key, 0xBE);
        assert_eq!(parse("f13").unwrap().key, 0x7C);
        assert_eq!(
            parse("win").unwrap(),
            Hotkey {
                modifiers: vec![],
                key: VK_LWIN
            }
        );
        assert!(parse("ctrl+a+b").is_err());
        assert!(parse("ctrl+nosuchkey").is_err());
    }

    #[test]
    fn names_round_trip() {
        for vk in [0x41, 0x35, 0x70, 0x87, 0x1B, 0x6B, 0x6D, 0xBE, 0xFF] {
            assert_eq!(vk_from_name(&name_from_vk(vk)), Some(vk), "vk {vk:#x}");
        }
        assert_eq!(format(true, true, false, false, 0x1B), "ctrl+shift+esc");
    }
}
