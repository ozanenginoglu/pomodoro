# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Pomodoro (binary `pomodoro`): a round, frameless desktop timer widget written in Rust (edition 2024) with GPUI. It has two modes, Pomodoro rounds and a meeting countdown, plus a small settings panel, desktop alerts and a `pomodoro <command>` CLI that talks to the running widget. It targets Linux (X11, developed under xmonad + picom, and Wayland) and Windows. User-facing usage, keys, CLI and the full `config.toml` format are in `README.md`; keep it in sync when you change any of them.

Code comments are in Turkish. Commit messages and the README are in English. All user-facing text goes through `src/i18n.rs` (English default, Turkish).

## Commands

```sh
cargo build --release                         # target/release/pomodoro
cargo clippy --all-targets -- -D warnings     # CI runs exactly this (plus --locked)
cargo test                                    # all unit tests
cargo test meeting::                          # one module's tests
cargo test parses_durations                   # single test by name substring

RUST_LOG=pomodoro=debug cargo run             # hover / command / restart logs; =trace adds pointer polling
POMODORO_DEMO=1 cargo run                     # debug builds only: Pomodoro minutes become seconds
POMODORO_SOCKET=test cargo run                # second instance next to the real one (separate IPC socket)
XDG_CONFIG_HOME=/tmp/x XDG_DATA_HOME=/tmp/x cargo run   # throwaway config dir and history file
```

- There is no `rustfmt.toml`, and the committed code is in a compact hand-formatted style. Do **not** run `cargo fmt` over the tree; it rewrites untouched files. Format only the files you changed: `rustfmt --edition 2024 --config use_small_heuristics=Max <files>`. rustfmt follows `mod` declarations, so passing `main.rs` or a `mod.rs` also reformats every module below it; check `git status` afterwards and restore files you didn't touch.
- The Windows target isn't installed locally (Arch `rust` package, no rustup). `src/platform/windows.rs` is only compiled and clippy-checked by CI (`.github/workflows/ci.yml`, Linux + Windows on every push to `main`; a `v*` tag publishes a GitHub release with both binaries). Mirror the exact `windows` 0.62 signatures from `~/.cargo/registry/src/*/windows-0.62.2` when writing Win32 calls.

### Testing GUI behavior without touching the user's session

The user's X session (`:0`, xmonad) must not be disturbed: don't move the real mouse or open test windows there. Use nested, invisible compositors instead, and run the app with its own `POMODORO_SOCKET`, `XDG_CONFIG_HOME` and `XDG_DATA_HOME`:

- **Wayland / layer-shell:** headless weston, with niri nested inside it: `weston --backend=headless --fake-seat --renderer=gl --socket=wl-pomo --idle-time=0`, then `WAYLAND_DISPLAY=wl-pomo niri -c <cfg>`. Run the app with `env -u DISPLAY WAYLAND_DISPLAY=wayland-1`. `--idle-time=0` matters: otherwise weston blanks after 5 minutes and nested niri freezes. Screenshots: `niri msg action screenshot-screen --path …`. Pointer input: a small `zwlr_virtual_pointer_v1` client; keyboard: `wtype`. (`ydotool` would move the real cursor.) Hyprland doesn't run nested in weston; niri also provides XWayland on `:1` for XWayland tests.
- **X11 / xmonad:** headless weston → rootful `Xwayland :5` → `Xephyr :6` (a plain X server without the XWAYLAND extension) → a minimal compiled xmonad config (`ghc -dynamic`, with `className =? "pomodoro" --> doFloat`), so the user's startup hooks don't run. Drive it with `xdotool` on `DISPLAY=:6`.
- Use `pkill -x <name>` to stop test processes; `pkill -f` patterns can match and kill the running shell. Never kill the user's own `~/.local/bin/pomodoro` instance; pick test instances by their `POMODORO_SOCKET` in `/proc/<pid>/environ`.

## Architecture

`main.rs` loads the config and sets the UI language first (so CLI messages are localized too). With an argument it acts as a client (`ipc::send`) and exits. Otherwise it claims the IPC socket (single instance), starts GPUI and opens the widget via `platform::open_widget`.

**Pure logic, no GPUI, fully unit-tested:**
- `timer.rs`: `Countdown` (Idle / Running / Paused), computed from start time with `Instant` injected, so tests don't sleep. `format_clock` rounds seconds up.
- `pomodoro.rs`: the work / short break / long break cycle on top of `Countdown`. `tick(now)` returns a `PhaseFinished` event; skipping a round doesn't count it.
- `meeting.rs`: `parse` (durations in English/Turkish units, `HH:MM` or `HH.MM` always means a clock time, 12 h cap) → `resolve` → `Meeting` with `Stage` (Normal < Warning < Critical < Overtime). `tick` reports each stage once.
- `history.rs`: appends finished/skipped rounds to `history.jsonl` in the data dir (one JSON line each, on a background thread). Rounds skipped before they ran aren't recorded.
- `status.rs`: `StatusInfo` → xmobar / waybar JSON / plain text.
- `config.rs`: serde + toml. `#[serde(default)]` everywhere, `normalized()` clamps to `limits`, atomic save, and a broken file is renamed to `.toml.broken`. `language` must stay the first field (TOML values must come before tables).
- `i18n.rs`: the active language is a process-global `AtomicU8`, so code outside the UI (alerts, the IPC thread, the CLI) can use `t()`. A new string needs a field in `Strings` and a value in **both** `EN` and `TR`; strings with arguments are functions at the bottom.

**UI (`ui/`):**
- `Entity<Config>` is the single source of truth. `settings.rs` changes it through `SettingsPanel::set` (clone → change → normalize → save → notify). `widget.rs` observes it in `apply_config` (pomodoro/meeting settings, notifications, size, position, language).
- `widget.rs` holds the modes and the timers. The ticker task only wakes at the next displayed-second boundary and only while some timer runs. Hover controls fade with a 200 ms animation driven by the widget's own 16 ms timer task (X11 doesn't deliver animation frames to unfocused windows). Hover-in comes from the `on_mouse_move` circle test. Hover-out depends on the platform: X11 polls the pointer via `platform::pointer_position` (GPUI's leave events are unreliable under xmonad), XWayland requires both the poll and `is_window_hovered` (the X pointer goes stale over native surfaces), and Wayland/Windows rely on `is_window_hovered`. `window_moved` saves a user-dragged position as `Position::Custom` (debounced; `moved_by_user` ignores WM nudges of a few pixels). `publish_status` writes the shared `ipc::Status` for `pomodoro status`.
- `garden.rs`: the tomato garden (pot + plant growing with today's work rounds), shown instead of the big clock while the Pomodoro timer is idle. Drawn with tiny-skia in a fixed design space (`VIEW`), cached by (pixels, rounds). `widget.rs` loads today's rounds from `history::work_rounds_on`, appends in `record_round` and resets at local midnight (`_midnight` task + `roll_garden` in render). Shared drawing helpers (`Painter`, `oval`, `rounded_rect`, …) live in `raster.rs`.
- `ring.rs`: the disc, track and progress arc are rasterized with tiny-skia at physical pixels and painted as a cached image, because GPUI paths aren't antialiased. It's drawn into the centered square of the bounds, so a non-square (e.g. tiled) window still shows a circle.

**Platform (`platform/`):** `mod.rs` holds shared window options, `widget_bounds` (position presets, custom position clamped to its display) and `moved_by_user`. `linux.rs` and `windows.rs` must export the same set: `open_widget`, `open_settings`, `apply_appearance` → `Applied`, `remembers_position`, `DRAG_TO_MOVE`, `is_xwayland`, `pointer_position`, `autostart`, `set_autostart`.
- Linux / Wayland: the widget and the settings panel are layer-shell surfaces when the compositor supports it. Otherwise (`LayerShellNotSupportedError`) they fall back to normal windows, which is why the build closures are `Fn`. A layer surface can't be moved, so position changes return `Applied::NeedsRestart`, and the widget calls `cx.restart()` once no timer is running.
- Linux / X11: x11rb for moving the window, EWMH `_NET_WM_STATE` (above, skip taskbar/pager) and pointer queries. The window class is `pomodoro` (settings: `pomodoro-settings`); the user's xmonad `doFloat` and picom `shadow-exclude` rules depend on it.
- Windows: `WindowKind::PopUp` (topmost tool window, no taskbar button). `SetWindowRgn` clips the widget to its circle and the settings window to its rounded rect. GPUI only removes the top frame when hiding the title bar, so regions and positions are offset by the client area's position inside the window rect (`client_area`). The circle's empty area is a native drag area (`WindowControlArea::Drag`); buttons use `.occlude()` so they stay clickable. Autostart uses the `HKCU\…\Run` registry value.

**Camera (`camera/`):** the break guard (`[camera] enabled`; `device = "none"` (`config::NO_CAMERA`) guards by keyboard/mouse input alone and never opens the camera). `camera::Watch` runs capture + face detection on its own thread only while a break is guarded; dropping it stops the camera. `frame.rs` samples any pixel format straight into the 320×240 model input, `detector.rs` runs the embedded UltraFace ONNX model with tract, and `PresenceFilter` debounces (present after 1 s, away after 3 s). Backends: `linux.rs` (V4L2 via `v4l`, device id = path) and `windows.rs` (Media Foundation via `nokhwa`, CI-only). While the camera runs, the widget counts the user present if it sees a face *or* there was keyboard/mouse input (`platform::user_active`); while it's starting or has failed, input alone decides. Frames are never stored; when testing, don't save or view camera frames, and don't change a device's parameters (v4l2loopback keeps them).

**IPC and alerts:** `ipc.rs` uses an `interprocess` local socket (abstract namespace on Linux, a named pipe on Windows). Single instance means "create fails but connect succeeds" (Linux reports `AddrInUse`, Windows `PermissionDenied`). Commands go to GPUI over an `async_channel`; `status [fmt]` is answered directly on the IPC thread from the shared `StatusInfo`. `alert.rs` fires notify-rust and a synthesized rodio chime on separate threads. `hooks.rs` runs the user's `[hooks]` shell commands one at a time on a single queue thread (ordering matters, e.g. `work_start`/`work_stop` toggling DND); `widget.rs` derives start/stop edges from `hooks::Activity` (held breaks count as running) and flushes the queue in `on_app_quit`.

**GPUI specifics:** the crates are `gpui-pre` / `gpui-pre-platform`, pinned to `=0.3.11` and renamed to `gpui` / `gpui_platform`. On Linux, `gpui_platform` needs `features = ["x11", "wayland"]`; without them the app silently runs headless. Mesa `MESA-EGL ... failed to create dri2 screen` warnings on NVIDIA at startup are harmless (wgpu probes GL before picking Vulkan).
