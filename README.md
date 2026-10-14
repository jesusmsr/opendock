# opendock

A lightweight controller for the AJAZZ AKP153E (Mirabox 293S V3) stream deck,
replacing the vendor "Stream Dock AJAZZ" app.

| | Vendor app | opendock |
|---|---|---|
| Processes | 20 | 1 |
| RAM | ~900 MB | ~10 MB (engine) |
| Install size | ~950 MB | ~9 MB |

## Layout

- `app/`: the `opendock` program
- `crates/ajazz-hid/`: the device's USB protocol, layout and image format
- `tools/probe/`: hardware test tool (test patterns, key event logging)

## Usage

Quit the vendor app first: only one program can drive the device.

Double-click `opendock.exe` to open the **settings window**:

- the 3x5 grid previews your keys exactly as the deck draws them; click one to
  edit it, drag to move or swap
- the editor sets the icon, an optional "active" icon, title, background and
  action (with a **Test** button); hotkeys can be recorded from the keyboard and
  audio devices are picked from a list of the ones connected
- pages can be added, renamed, deleted and set as home
- **⚙ Settings**: start opendock with Windows, deck brightness, title font
- **Save** (Ctrl+S) writes the config; a running engine applies it immediately
- **Start/Stop** at the bottom controls the engine; ☰ → *Import from Stream Dock
  AJAZZ* converts the vendor app's profile

The **engine** (`opendock run`) is what drives the deck. It lives in the
notification area: double-click its icon to open the settings, right-click for
*Open settings*, *Start with Windows* and *Quit*. It uses about 10-18 MB of memory.
"Start with Windows" adds a per-user startup entry (no admin rights needed).

From a terminal:

```
opendock run         # start the engine
opendock import      # one-time: convert the vendor app's active profile
opendock check       # validate the config
opendock preview out # render every key to PNG
opendock autostart on|off|status
opendock audio       # list audio devices
opendock audio output "Headset"   # set the default output from a script
```

The config is `%APPDATA%\opendock\config.toml` (override with `--config <file>`).
The log is `opendock.log` next to it.

## Config

```toml
brightness = 60          # 0-100
home = "main"            # first page shown
# font = "C:/Windows/Fonts/segoeuib.ttf"

[[pages.main]]
pos = "1,1"              # row,col from the top-left, 1-based (3 rows x 5 cols)
icon = "icons/game.png"  # relative to the config folder; png or jpg
icon_active = "icons/game-on.png"  # optional; shown while the key is active
title = "Gaming"         # optional; "\n" for a second line
color = "#202040"        # optional background
title_size = 16          # optional
title_align = "bottom"   # top | middle | bottom
action = { run = "C:/scripts/gaming-mode.bat" }
```

Actions:

| Action | Example |
|---|---|
| Run a program, script, file or folder | `{ run = "C:/tools/app.exe" }` |
| ...with options | `{ run = { path = "x.bat", args = "-q", cwd = "C:/", hidden = true } }` |
| Open a URL | `{ url = "https://example.com" }` |
| Hotkey | `{ hotkey = "ctrl+shift+esc" }`, `{ hotkey = "win+numminus" }` |
| Media key | `{ media = "play_pause" }`: `next`, `previous`, `stop`, `mute`, `volume_up`, `volume_down` |
| Type text | `{ text = "Hello!" }` |
| Default audio output | `{ audio_output = "Headset" }` |
| Cycle audio outputs | `{ audio_output = ["Headset", "Speakers"] }` |
| Default microphone | `{ audio_input = "Webcam" }` |
| Go to a page | `{ page = "obs" }` |
| Previous page / home page | `"back"`, `"home"` |
| Several steps | `{ multi = [{ hotkey = "win+e" }, { delay = 800 }, { hotkey = "ctrl+e" }] }` |

Audio devices are matched by exact id, then exact name, then any part of the
name (case-insensitive); `opendock audio` lists the names. A key that selects a
single device is **active** while that device is the default: it shows
`icon_active` if set, otherwise a green border. Switching sets the device for
all roles (media, games and calls).

Hotkey names: letters, digits, `f1`-`f24`, `esc`, `enter`, `tab`, `space`,
`backspace`, `delete`, `insert`, `home`, `end`, `pageup`, `pagedown`,
`up`/`down`/`left`/`right`, `printscreen`, `num0`-`num9`, `numplus`,
`numminus`, `nummultiply`, `numdivide`, `numdecimal`, `period`, `comma`,
`minus`, `plus`, `slash`, `semicolon`, `quote`, `backquote`, `lbracket`,
`rbracket`, `backslash`, or `vk:0xNN` for any other virtual-key code.
Modifiers: `ctrl`, `shift`, `alt`, `win`.

## Building

Needs Rust (MSVC toolchain) and the Visual Studio C++ Build Tools.

```
cargo build --release   # target/release/opendock.exe
cargo test --workspace
```
