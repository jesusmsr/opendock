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

impl Default for Config {
    /// An empty home page: a valid starting point for new users.
    fn default() -> Self {
        Config {
            brightness: default_brightness(),
            home: default_home(),
            font: None,
            pages: HashMap::from([(default_home(), Vec::new())]),
        }
    }
}

impl Key {
    pub fn empty(pos: Pos) -> Self {
        Key {
            pos,
            icon: None,
            icon_active: None,
            title: None,
            color: None,
            title_size: None,
            title_align: None,
            action: None,
        }
    }
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
    /// Icon shown instead of `icon` while the key is active (e.g. its audio
    /// device is the current default). Without it, active keys get a green border.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_active: Option<PathBuf>,
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
    /// Make a playback device the default; a list cycles through devices.
    AudioOutput(AudioTarget),
    /// Make a recording device the default; a list cycles through devices.
    AudioInput(AudioTarget),
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

/// Device name (or part of it), device id, or a list of them to cycle through.
#[derive(Debug, Clone, Deserialize, Serialize, Hash)]
#[serde(untagged)]
pub enum AudioTarget {
    One(String),
    Cycle(Vec<String>),
}

impl AudioTarget {
    pub fn targets(&self) -> Vec<String> {
        match self {
            AudioTarget::One(s) => vec![s.clone()],
            AudioTarget::Cycle(v) => v.clone(),
        }
    }

    fn to_inline_toml(&self) -> String {
        match self {
            AudioTarget::One(s) => quote(s),
            AudioTarget::Cycle(v) => {
                format!(
                    "[{}]",
                    v.iter().map(|s| quote(s)).collect::<Vec<_>>().join(", ")
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Hash)]
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
            Action::AudioOutput(t) => format!("{{ audio_output = {} }}", t.to_inline_toml()),
            Action::AudioInput(t) => format!("{{ audio_input = {} }}", t.to_inline_toml()),
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
                if let Some(i) = &k.icon_active {
                    out += &format!(
                        "icon_active = {}\n",
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

    /// Writes the config atomically (temp file + rename), so the running
    /// engine never reads a half-written file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, self.to_toml()).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Renames a page and every link to it. Returns false if `to` is taken.
    pub fn rename_page(&mut self, from: &str, to: &str) -> bool {
        if from == to || self.pages.contains_key(to) {
            return from == to;
        }
        let Some(keys) = self.pages.remove(from) else {
            return false;
        };
        self.pages.insert(to.to_string(), keys);
        if self.home == from {
            self.home = to.to_string();
        }
        fn relink(a: &mut Action, from: &str, to: &str) {
            match a {
                Action::Page(p) if p == from => *p = to.to_string(),
                Action::Multi(list) => list.iter_mut().for_each(|a| relink(a, from, to)),
                _ => {}
            }
        }
        for key in self.pages.values_mut().flatten() {
            if let Some(a) = &mut key.action {
                relink(a, from, to);
            }
        }
        true
    }

    pub fn validate(&self) -> Result<(), String> {
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
                    self.check_action(a)
                        .map_err(|e| format!("page {name:?}, key {}: {e}", k.pos))?;
                }
            }
        }
        Ok(())
    }

    /// Checks what can be checked without running the action.
    fn check_action(&self, a: &Action) -> Result<(), String> {
        match a {
            Action::Page(p) if !self.pages.contains_key(p) => {
                Err(format!("links to undefined page {p:?}"))
            }
            Action::Hotkey(h) => crate::keys::parse(h).map(|_| ()),
            Action::Multi(list) => list.iter().try_for_each(|a| self.check_action(a)),
            _ => Ok(()),
        }
    }
}

/// `%APPDATA%\opendock\config.toml`
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| ".".into());
    base.join("opendock").join("config.toml")
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
            [[pages.other]]
            pos = "1,1"
            icon_active = "icons/on.png"
            action = { audio_output = ["Headset", "Speakers"] }
            [[pages.other]]
            pos = "1,2"
            action = { audio_input = "Microphone" }
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
    fn rename_page_updates_links_and_home() {
        let mut cfg: Config = toml::from_str(
            r#"
            [[pages.main]]
            pos = "1,1"
            action = { multi = [{ page = "obs" }, "back"] }
            [[pages.obs]]
            pos = "1,1"
            action = { page = "main" }
            "#,
        )
        .unwrap();
        assert!(cfg.rename_page("main", "home"));
        assert!(cfg.rename_page("obs", "stream"));
        assert!(!cfg.rename_page("stream", "home"), "target name is taken");
        assert_eq!(cfg.home, "home");
        cfg.validate().unwrap();
        assert!(cfg.to_toml().contains(r#"{ page = "stream" }"#));
        assert!(cfg.to_toml().contains(r#"{ page = "home" }"#));
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
