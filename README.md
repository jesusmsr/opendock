# opendock

A lightweight controller for the AJAZZ AKP153E (Mirabox 293S V3) stream deck,
replacing the vendor "Stream Dock AJAZZ" app.

| | Vendor app | opendock |
|---|---|---|
| Processes | 20 | 1 |
| RAM | ~900 MB | ~8 MB |
| Install size | ~950 MB | ~1.5 MB |

## Layout

- `app/`: the `opendock` program
- `crates/ajazz-hid/`: the device's USB protocol, layout and image format
- `tools/probe/`: hardware test tool (test patterns, key event logging)

## Usage

Quit the vendor app first: only one program can drive the device.

```
opendock import      # one-time: convert the vendor app's active profile
opendock check       # validate the config
opendock             # run (reconnects automatically, reloads config on save)
opendock preview out # render every key to PNG
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
| Go to a page | `{ page = "obs" }` |
| Previous page / home page | `"back"`, `"home"` |
| Several steps | `{ multi = [{ hotkey = "win+e" }, { delay = 800 }, { hotkey = "ctrl+e" }] }` |

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
