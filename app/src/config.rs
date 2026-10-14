//! `config.toml`: pages of keys, each with an optional icon, title and action.
//!
//! ```toml
//! brightness = 60
//! home = "main"
//!
//! [[pages.main]]
//! pos = "1,1"                      # row,col counted from the top-left, 1-based
//! icon = "icons/gaming.png"        # relative to the config file's folder
//! title = "Gaming"
//! action = { run = "C:/scripts/gaming-mode.bat" }
//! ```

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Screen brightness, 0-100.
    #[serde(default = "default_brightness")]
    pub brightness: u8,
    /// Page shown at start-up and by the `home` action.
    #[serde(default = "default_home")]
    pub home: String,
    /// TrueType font for titles. Defaults to Segoe UI Bold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<PathBuf>,
    #[serde(default)]
    pub pages: HashMap<String, Vec<Key>>,
}

fn default_brightness() -> u8 {
    60
}

fn default_home() -> String {
    "main".into()
}

#[derive(Debug, Clone, Deserialize, Serialize, Hash)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub pos: Pos,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Background colour as `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_align: Option<Align>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Top,
    Middle,
    Bottom,
}

/// 0-based (row, col), written as 1-based `"row,col"` in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pos {
    pub row: u8,
    pub col: u8,
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.row + 1, self.col + 1)
    }
}

impl<'de> Deserialize<'de> for Pos {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let bad = || {
            serde::de::Error::custom(format!(
                "pos must look like \"row,col\" (1-based), got {s:?}"
            ))
        };
        let (r, c) = s.split_once(',').ok_or_else(bad)?;
        let r: u8 = r.trim().parse().map_err(|_| bad())?;
        let c: u8 = c.trim().parse().map_err(|_| bad())?;
        if r == 0 || c == 0 {
            return Err(bad());
        }
        Ok(Pos {
            row: r - 1,
            col: c - 1,
        })
    }
}

impl Serialize for Pos {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

/// What a key does when pressed. Written as e.g. `{ hotkey = "win+d" }` or `"back"`.
#[derive(Debug, Clone, Deserialize, Serialize, Hash)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Launch a program, script, document or folder.
    Run(Run),
    /// Open a URL in the default browser.
    Url(String),
    /// Key combination such as `"ctrl+shift+esc"` or `"win+numminus"`.
    Hotkey(String),
    Media(Media),
    /// Type text.
    Text(String),
    /// Go to a page.
    Page(String),
    /// Return to the previous page.
    Back,
    /// Go to the home page.
    Home,
    /// Pause inside a `multi` action, in milliseconds.
    Delay(u64),
    /// Run several actions in order.
    Multi(Vec<Action>),
}

#[derive(Debug, Clone, Deserialize, Serialize, Hash)]
#[serde(untagged)]
pub enum Run {
    Path(String),
    Full {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        /// Hide the window (useful for .bat files).
        #[serde(default)]
        hidden: bool,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Media {
    PlayPause,
    Next,
    Previous,
    Stop,
    Mute,
    VolumeUp,
    VolumeDown,
}

impl Media {
    pub fn label(self) -> &'static str {
        match self {
            Media::PlayPause => "Play/Pause",
            Media::Next => "Next",
            Media::Previous => "Previous",
            Media::Stop => "Stop",
            Media::Mute => "Mute",
            Media::VolumeUp => "Vol +",
            Media::VolumeDown => "Vol -",
        }
    }
}

/// A TOML string literal.
fn quote(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

impl Action {
    /// One-line TOML form, e.g. `{ hotkey = "win+d" }`.
    pub fn to_inline_toml(&self) -> String {
        match self {
            Action::Run(Run::Path(p)) => format!("{{ run = {} }}", quote(p)),
            Action::Run(Run::Full {
                path,
                args,
                cwd,
                hidden,
            }) => {
                let mut parts = vec![format!("path = {}", quote(path))];
                if let Some(a) = args {
                    parts.push(format!("args = {}", quote(a)));
                }
                if let Some(c) = cwd {
                    parts.push(format!("cwd = {}", quote(c)));
                }
                if *hidden {
                    parts.push("hidden = true".into());
                }
                format!("{{ run = {{ {} }} }}", parts.join(", "))
            }
            Action::Url(u) => format!("{{ url = {} }}", quote(u)),
            Action::Hotkey(h) => format!("{{ hotkey = {} }}", quote(h)),
            Action::Media(m) => {
                let name = toml::Value::try_from(m).expect("media serializes to a string");
                format!("{{ media = {name} }}")
            }
            Action::Text(t) => format!("{{ text = {} }}", quote(t)),
            Action::Page(p) => format!("{{ page = {} }}", quote(p)),
            Action::Back => "\"back\"".into(),
            Action::Home => "\"home\"".into(),
            Action::Delay(ms) => format!("{{ delay = {ms} }}"),
            Action::Multi(list) => {
                let items: Vec<String> = list.iter().map(Action::to_inline_toml).collect();
                format!("{{ multi = [{}] }}", items.join(", "))
            }
        }
    }
}

impl Config {
    /// Hand-formatted TOML: one `[[pages.<name>]]` block per key, actions inline.
    pub fn to_toml(&self) -> String {
        let mut out = format!(
            "brightness = {}\nhome = {}\n",
            self.brightness,
            quote(&self.home)
        );
        if let Some(f) = &self.font {
            out += &format!("font = {}\n", quote(&f.to_string_lossy()));
        }
        let mut names: Vec<&String> = self.pages.keys().collect();
        names.sort_by_key(|n| (*n != &self.home, n.as_str()));
        for name in names {
            let mut keys = self.pages[name].clone();
            keys.sort_by_key(|k| (k.pos.row, k.pos.col));
            let table = if name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                name.clone()
            } else {
                quote(name)
            };
            for k in keys {
                out += &format!("\n[[pages.{table}]]\npos = \"{}\"\n", k.pos);
                if let Some(i) = &k.icon {
                    out += &format!(
                        "icon = {}\n",
                        quote(&i.to_string_lossy().replace('\\', "/"))
                    );
                }
                if let Some(t) = &k.title {
                    out += &format!("title = {}\n", quote(t));
                }
                if let Some(c) = &k.color {
                    out += &format!("color = {}\n", quote(c));
                }
                if let Some(s) = k.title_size {
                    out += &format!("title_size = {s}\n");
                }
                if let Some(a) = k.title_align {
                    let a = toml::Value::try_from(a).expect("align serializes to a string");
                    out += &format!("title_align = {a}\n");
                }
                if let Some(a) = &k.action {
                    out += &format!("action = {}\n", a.to_inline_toml());
                }
            }
        }
        out
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let cfg: Config = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<(), String> {
        if !self.pages.contains_key(&self.home) {
            return Err(format!(
                "home page {:?} is not defined under [pages]",
                self.home
            ));
        }
        for (name, keys) in &self.pages {
            let mut seen = std::collections::HashSet::new();
            for k in keys {
                if ajazz_hid::akp153e::key_id(k.pos.row, k.pos.col).is_none() {
                    return Err(format!(
                        "page {name:?}: pos {} is outside the 3x5 grid",
                        k.pos
                    ));
                }
                if !seen.insert(k.pos) {
                    return Err(format!("page {name:?}: pos {} is used twice", k.pos));
                }
                if let Some(a) = &k.action {
                    self.check_pages(name, a)?;
                }
            }
        }
        Ok(())
    }

    fn check_pages(&self, page: &str, a: &Action) -> Result<(), String> {
        match a {
            Action::Page(p) if !self.pages.contains_key(p) => {
                Err(format!("page {page:?}: links to undefined page {p:?}"))
            }
            Action::Multi(list) => list.iter().try_for_each(|a| self.check_pages(page, a)),
            _ => Ok(()),
        }
    }
}

/// `%APPDATA%\chechudock\config.toml`
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| ".".into());
    base.join("chechudock").join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_action_forms() {
        let cfg: Config = toml::from_str(
            r#"
            [[pages.main]]
            pos = "1,1"
            action = { run = "a.bat" }
            [[pages.main]]
            pos = "1,2"
            action = { run = { path = "b.bat", hidden = true } }
            [[pages.main]]
            pos = "3,5"
            action = { multi = [{ hotkey = "win+d" }, { delay = 100 }, "back", { media = "play_pause" }] }
            [[pages.main]]
            pos = "2,1"
            action = { page = "main" }
            "#,
        )
        .unwrap();
        cfg.validate().unwrap();
        let keys = &cfg.pages["main"];
        assert_eq!(keys[0].pos, Pos { row: 0, col: 0 });
        assert!(matches!(
            keys[1].action,
            Some(Action::Run(Run::Full { hidden: true, .. }))
        ));
        assert!(matches!(&keys[2].action, Some(Action::Multi(v)) if v.len() == 4));
    }

    #[test]
    fn to_toml_round_trips() {
        let src = r#"
            brightness = 40
            [[pages.main]]
            pos = "1,1"
            icon = "icons/a.png"
            title = "Say \"hi\""
            action = { multi = [{ hotkey = "win+d" }, { delay = 100 }, { run = { path = "b.bat", hidden = true } }, { media = "play_pause" }, { page = "other" }] }
            [[pages.other]]
            pos = "3,5"
            title_align = "top"
            action = "back"
        "#;
        let cfg: Config = toml::from_str(src).unwrap();
        let again: Config = toml::from_str(&cfg.to_toml()).unwrap();
        again.validate().unwrap();
        assert_eq!(cfg.to_toml(), again.to_toml());
        assert!(cfg
            .to_toml()
            .contains("action = { multi = [{ hotkey = \"win+d\" }"));
    }

    #[test]
    fn rejects_bad_layout() {
        let off_grid: Config = toml::from_str("[[pages.main]]\npos = \"4,1\"").unwrap();
        assert!(off_grid.validate().is_err());
        let dangling: Config =
            toml::from_str("[[pages.main]]\npos = \"1,1\"\naction = { page = \"nope\" }").unwrap();
        assert!(dangling.validate().is_err());
        assert!(toml::from_str::<Config>("[[pages.main]]\npos = \"0,1\"").is_err());
    }
}
