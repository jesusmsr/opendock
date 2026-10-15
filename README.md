# opendock

A lightweight replacement for the vendor "Stream Dock AJAZZ" app, for the
AJAZZ AKP153E (Mirabox 293S V3) stream deck on Windows.

Original software was hard to find and worked not so well so I decided to make my own.

It is one small program with a settings window and a tray icon: the engine that
drives the deck uses about 10 MB of memory, against ~900 MB and 20 processes
for the vendor app.

## Quickstart

You need Rust (MSVC toolchain) and the Visual Studio C++ Build Tools.

```
cargo build --release
```

1. Quit the vendor app. Only one program can drive the deck at a time.
2. Run `target\release\opendock.exe` to open the settings window.
3. Optional: in the ☰ menu, choose *Import from Stream Dock AJAZZ* to convert
   your existing profile.
4. Click a key to set its icon, title and action, then press **Save** (Ctrl+S).
5. Press **Start** to run the engine. It stays in the notification area and
   applies every change you save.

Turn on *Start with Windows* in ⚙ Settings or in the tray menu to have it
running at sign-in.

The config is `%APPDATA%\opendock\config.toml`, with the log next to it.

## Features

- **Visual editor**: a live 3x5 preview of the deck; drag to move or swap
  keys, copy and paste, undo and redo, and a Test button for every action
- **Pages**: any number of pages, with actions to go to a page, back or home
- **Actions**: run programs, scripts, files or folders, open URLs, send
  hotkeys (recorded from the keyboard), media keys and typed text, and chain
  several steps with delays
- **Audio switching**: set or cycle the default output or microphone; the key
  shows an active icon (or a green border) while its device is the default
- **Small screen widgets**: up to 4 stacked widgets for CPU and GPU
  temperature and usage, VRAM, RAM, clock, date or custom text
- **Icons and layout**: png and jpg icons with a separate active icon, titles,
  background colours, font choice and brightness
- **Import**: converts the vendor app's profile, including its small-screen
  widgets
- **Always on**: one instance of the engine and one settings window, an
  autostart entry that needs no admin rights, and automatic reconnection when
  the deck is unplugged
- **Command line**: `run`, `check`, `import`, `preview`, `audio` and
  `autostart` for scripting
