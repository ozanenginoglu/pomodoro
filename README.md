# Pomodoro

A small, round timer that sits on your desktop. It tracks your work and break rounds (Pomodoro), and in meetings it counts down to the end time. You only see a circle: no title bar, no window frame. The controls fade in when you hover over it and fade out when you leave.

Runs on **Linux (X11 and Wayland)** and **Windows**. It's written in Rust with [GPUI](https://www.gpui.rs). The interface is in English (default) and Turkish; you can switch between them in the settings panel.

![Five widgets in a row on a dark background: a focus round with 18 minutes left and a pink ring; the same round on hover, with the settings and meeting icons above the clock and reset, pause and skip buttons below it; a break with a green ring; a meeting in its warning stage, 3 minutes left, with a yellow ring and the end time under the clock; and a meeting running over time, counting up in red](docs/widgets.png)

| | |
|---|---|
| **Pomodoro** | Work, short break and long break rounds. The dots under the clock show where you are in the cycle. Breaks can start on their own when a work round ends. |
| **Tomato garden** | While the timer is stopped, a little pot shows today's work: every finished round adds a tomato to the plant, and the plant grows through the day. |
| **Break guard** | Optional. A break only counts down while you're away from the computer. While the camera sees you at the desk (or, without a camera, while you use the keyboard and mouse), it waits, and a little tomato on the widget glares at you until you step away. |
| **Meeting** | Type a length (`45`, `1h30`) or an end time (`14:30`) and it counts down. The ring turns yellow, then orange as the end gets close, and red when the meeting runs over. |
| **Alerts** | A desktop notification and a short bell chime at the end of each round and at each meeting stage. |
| **Status bar** | `pomodoro status` prints the current state for xmobar, waybar or plain text. |
| **Hooks** | Run your own commands when a round starts or ends or a meeting changes stage: silence notifications while you work, send the "break's over" alert to your phone, color your keyboard. |

![The settings panel: work, short break and long break lengths, rounds before a long break, auto-start switches, the break guard switch and camera picker, meeting display and warning times, notification and sound switches, and the widget's size, position, always-on-top setting and language](docs/settings.png)

## Install

Prebuilt binaries for Linux (x86_64) and Windows are attached to each release on the [Releases](https://github.com/ozanenginoglu/pomodoro/releases) page.

- **Linux:** make the binary executable and put it on your `PATH`, e.g. `install -Dm755 pomodoro-linux-x86_64.bin ~/.local/bin/pomodoro`.
- **Windows:** run `pomodoro-windows-x86_64.exe`. The binary isn't code-signed, so SmartScreen may warn on first launch. Choose *More info → Run anyway*.

With a Rust toolchain (1.88 or newer) you can also install from source:

```sh
cargo install --git https://github.com/ozanenginoglu/pomodoro
```

## Build

```sh
cargo build --release
./target/release/pomodoro
```

On Linux you need the development packages for ALSA, fontconfig, FreeType, Wayland, xkbcommon and Vulkan:

```sh
# Arch / CachyOS
sudo pacman -S --needed alsa-lib fontconfig freetype2 wayland libxkbcommon libxkbcommon-x11 vulkan-icd-loader

# Debian / Ubuntu
sudo apt install pkg-config libasound2-dev libfontconfig-dev libfreetype-dev libwayland-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libx11-xcb-dev libxcb1-dev libvulkan-dev
```

On Windows, the MSVC toolchain needs nothing extra.

## Usage

Run `pomodoro` to open the widget. Only one widget runs at a time; starting it again does nothing. To close it, use **Quit Pomodoro** at the bottom of the settings panel or run `pomodoro quit`. The timers aren't saved, so a running round is lost.

Hover over the circle to show the controls:

- **Top row:** the gear opens the settings panel, the other icon switches between Pomodoro and meeting mode.
- **Bottom row:** reset, start/pause and skip (Pomodoro), or end (meeting).

### Keyboard

Click the widget first so it has keyboard focus.

| Key | Pomodoro | Meeting |
|---|---|---|
| `Space` | Start / pause | — |
| `R` | Restart the round (when stopped: reset the whole cycle) | End the meeting |
| `S` | Skip to the next round | — |
| `M` or `Tab` | Switch to meeting mode | Switch to Pomodoro mode |
| `Ctrl+,` | Open settings | Open settings |

While you're typing a meeting length, letters go into the field, so only `Tab` and `Ctrl+,` work.

### Starting a meeting

Switch to meeting mode, type how long the meeting is or when it ends, and press `Enter`. `Backspace` deletes, `Esc` clears.

| You type | Meaning |
|---|---|
| `45`, `45m`, `45 min`, `45dk` | 45 minutes from now |
| `1h30`, `1 hour 30 min`, `1sa30`, `1 saat 30 dk` | 1 hour 30 minutes from now |
| `14:30` or `14.30` | Ends at 14:30 (tomorrow, if that time has already passed today) |

A meeting can be up to 12 hours long. Pomodoro keeps running in the background while you're in meeting mode, and the other way round.

### Tomato garden

While the Pomodoro timer is stopped (between rounds, or at the end of the day), the widget shows a flower pot above a smaller clock. Every work round you finish today hangs a red tomato on the plant; a round you skipped after it started stays a small green one. The plant grows with the day: a sprout in the morning, a full tomato plant after six rounds. The tomato from your latest round smiles at you. The plant holds 12 tomatoes; on longer days a small badge on top shows how many rounds you finished.

The garden comes from the [history file](#history), so it survives a restart. At midnight it starts again from a sprout. While a round or break is counting, the widget shows the plain clock.

### Break guard

The break guard is off until you switch on *Pause breaks while I'm at the desk* in the settings panel. Pick a camera with the arrows next to *Camera*, or choose *None* if you don't want the camera to watch you: the guard then goes by keyboard and mouse alone, and the camera is never opened. When a break starts, a little tomato appears on the widget. The break only counts down while you're away:

![Three widgets during a short break: the tomato smiling and looking down at the cursor while the break waits; the tomato dark red and angry, frowning and gritting its teeth with steam rising from its top; and the tomato calm and smiling again while the break counts down](docs/break-guard.png)

- **You're still at the desk** (the camera sees your face, or you typed or moved the mouse in the last 10 seconds): the break waits. The tomato looks at you (or at your cursor) and, after a few seconds, gets angry: it turns dark red, frowns, grits its teeth and steams.
- **You stepped away** (no face for 3 seconds and no keyboard or mouse input): the tomato calms down, sways gently and looks around, and the break counts down.

So a 5-minute break ends after 5 minutes away from the computer, and those minutes have to be in one piece:

- If you're still at the desk when the break starts, it simply waits for you to leave.
- Once the break has started counting, coming back for less than 25 seconds (to dismiss a notification, say) only pauses it.
- Coming back for 25 seconds or more restarts the break from its full length. It counts down again once you leave.

You can still pause the break or skip it with the controls.

- With *None*, sitting still at the desk counts as away, because only keyboard and mouse input shows that you're there.
- The camera is on only during a guarded break and turns off when the break ends, is paused or is skipped.
- Frames are processed in memory, about two per second, and never saved or sent anywhere. The detector only finds faces; it doesn't recognize who you are.
- A face has to be visible for 1 second to count as present, and gone for 3 seconds to count as away, so briefly turning your head doesn't start the break.
- If the camera can't be opened or stops sending frames, the guard goes by keyboard and mouse alone (sitting still then counts as away) and tries the camera again every 10 seconds.

Faces are found with the [UltraFace](https://github.com/Linzaer/Ultra-Light-Fast-Generic-Face-Detector-1MB) RFB-320 model (MIT, built into the binary) running on [tract](https://github.com/sonos/tract). Cameras are read through V4L2 on Linux (YUYV, NV12, I420, MJPEG, grey and RGB; loopback devices such as DroidCam work too) and Media Foundation on Windows. Keyboard and mouse activity comes from the system's idle time: the MIT-SCREEN-SAVER extension on X11, the `ext-idle-notify-v1` protocol on Wayland and `GetLastInputInfo` on Windows. The tomato looks at your face when the camera sees it; otherwise it follows the cursor on X11 and Windows and looks straight ahead on Wayland, because Wayland doesn't tell apps where the cursor is.

### Command line

The widget listens on a local socket (a Unix socket on Linux, a named pipe on Windows), so you can control it from scripts and key bindings. Messages are printed in the language set in the settings:

```
pomodoro              open the widget
pomodoro toggle       start / pause the Pomodoro timer
pomodoro reset        restart the current round
pomodoro skip         skip to the next round
pomodoro mode         switch between Pomodoro and meeting mode
pomodoro settings     open the settings panel
pomodoro quit         close the widget
pomodoro status [--format xmobar|waybar|plain]
                      print the current state (empty if the widget isn't running)
```

### Status bar

**xmobar** (the default on Linux). The icons come from Font Awesome Solid, which the output selects with `<fn=4>`, so make it the fourth entry in `additionalFonts`:

```haskell
additionalFonts = [ font1, font2, font3, "Font Awesome 6 Free Solid 10" ]
commands = [ ..., Run Com "pomodoro" ["status"] "pomo" 10 ]
template = "... %pomo% ..."
```

**waybar** (`--format waybar` prints JSON):

```jsonc
"custom/pomodoro": {
    "exec": "pomodoro status --format waybar",
    "return-type": "json",
    "interval": 1,
    "on-click": "pomodoro toggle"
}
```

The CSS class is one of `idle`, `work`, `break`, `paused`, `meeting`, `warning`, `critical`, `overtime` (and `closed` when the widget isn't running, with empty text so waybar hides the module).

### Hooks

Hooks run a shell command of your choice when something happens: `sh -c` on Linux, `cmd /C` on Windows. Add them to the `[hooks]` table in the [config file](#configuration). There's no setting for them in the panel. The widget reads the file when it starts, so quit it before you edit the file and start it again afterwards; otherwise the settings panel may overwrite your changes.

```toml
[hooks]
# Pause desktop notifications (dunst) while you work, show them again when you stop.
work_start = "dunstctl set-paused true"
work_stop  = "dunstctl set-paused false"

# The break guard makes you leave the desk, so send "break's over" to your phone (ntfy.sh).
break_done = 'curl -s -d "$POMODORO_TITLE: $POMODORO_MESSAGE" ntfy.sh/my-pomodoro-topic'
# ...or through KDE Connect:
# break_done = 'kdeconnect-cli -n "My phone" --ping-msg "$POMODORO_MESSAGE"'
```

| Hook | Runs when |
|---|---|
| `work_start`, `break_start` | A work round or break starts counting: you started or resumed it, or it started on its own after the previous round. A break the break guard is holding still counts as started. |
| `work_stop`, `break_stop` | It stops counting: it finished, you paused, reset or skipped it, or you quit the widget. |
| `work_done`, `break_done` | It ran its full length. This is when the desktop notification appears. |
| `meeting_start` | You start a meeting. |
| `meeting_warning`, `meeting_critical`, `meeting_overtime` | The meeting reaches that stage (the same moments as the notifications). |
| `meeting_end` | You end the meeting, or you quit the widget during one. |

The command gets these environment variables (`%NAME%` in `cmd` on Windows):

| Variable | Value |
|---|---|
| `POMODORO_EVENT` | The hook's name, e.g. `work_start`. |
| `POMODORO_PHASE` | `work`, `short_break` or `long_break`. For `*_done`, the round that finished. |
| `POMODORO_NEXT` | `*_done` only: the round that comes next. |
| `POMODORO_REASON` | Start: `started`, `resumed` or `auto`. Stop: `finished`, `paused`, `reset`, `skipped` or `quit`. `meeting_end`: `ended` or `quit`. |
| `POMODORO_MINUTES` | Start and `meeting_start`: minutes left. |
| `POMODORO_TITLE`, `POMODORO_MESSAGE` | `*_done` and the meeting stages: the notification's text, in the language set in the settings. |
| `POMODORO_MEETING_END` | Meeting hooks: the end time, e.g. `14:30`. |

Hooks run one at a time, in the order of the events, so a `work_stop` never overtakes the `work_start` before it. A hook that runs longer than 30 seconds is stopped; end a long-running command with `&` to keep it going in the background. When you quit the widget, it waits up to 2 seconds for pending hooks. A hook that fails is only logged (`RUST_LOG=pomodoro=info pomodoro` shows each hook as it runs).

### History

Every round that runs to the end or that you skip after it started is added as one line to a history file (the tomato garden reads today's work rounds from it):

- Linux: `~/.local/share/pomodoro/history.jsonl`
- Windows: `%APPDATA%\pomodoro\data\history.jsonl`

```json
{"end":"2026-11-09T14:30:00+03:00","phase":"work","outcome":"done","seconds":1500}
```

`phase` is `work`, `short_break` or `long_break`; `outcome` is `done` or `skipped`; `seconds` is how long the round ran. The file only grows. You can delete or trim it at any time.

## Platform notes

### Linux (X11)

The widget needs a compositor (e.g. **picom**) for the transparent corners; without one they show up black. Tiling window managers should float it and the settings panel. For xmonad and picom:

```haskell
-- xmonad.hs, in your manageHook (doCenterFloat is from XMonad.Hooks.ManageHelpers)
className =? "pomodoro"          --> doFloat
className =? "pomodoro-settings" --> doCenterFloat
```

```
# picom.conf
shadow-exclude = [ "class_g = 'pomodoro'" ];
```

The window class is `pomodoro` (the settings panel is `pomodoro-settings`). To start the widget when you log in, run it from your startup hook, e.g. `spawnOnce "pomodoro"` in xmonad. If you move the widget with your window manager (xmonad: mod + drag), its new position is saved.

### Linux (Wayland)

On compositors with the layer-shell protocol (Hyprland, Sway, niri, KDE Plasma, …) the widget is a real desktop widget: it stays at the chosen corner, has no taskbar entry, and stays above windows (or below them, with *always on top* turned off). The settings panel opens in the middle of the screen. Wayland doesn't let a window place itself, so a new position takes effect after a quick restart. To keep your progress, the restart waits until nothing would be lost: no meeting, and the Pomodoro cycle back at the start of its first round (press `R` while stopped to reset it). Switching back to the original position cancels the restart.

On GNOME (no layer-shell) the widget opens as a normal window and the compositor decides where it goes.

### Windows

The widget stays on top, has no taskbar button, and clicks on the transparent corners go through to the window behind it. Drag the circle to move it; the position is remembered. **Start on sign-in** in the settings panel adds the widget to `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

## Configuration

Settings are saved as you change them in the settings panel. The file lives at:

- Linux: `~/.config/pomodoro/config.toml`
- Windows: `%APPDATA%\pomodoro\config\config.toml`

Every key is optional. These are the defaults:

```toml
language = "en"                 # "en" (English) or "tr" (Türkçe)

[pomodoro]
work_minutes = 25               # 1–180
short_break_minutes = 5         # 1–60
long_break_minutes = 15         # 1–60
rounds_before_long_break = 4    # 1–12
auto_start_breaks = true
auto_start_work = false

[camera]
enabled = false                 # break guard: breaks only count down while you're away
# device = "/dev/video0"        # Linux: device path; Windows: set by the settings panel. Unset: the first camera; "none": keyboard and mouse only

[meeting]
display = "both"                # "remaining", "end_time" or "both"
warning_minutes = 5             # 1–30, the ring turns yellow
critical_minutes = 1            # 1–10, the ring turns orange

[notifications]
enabled = true
sound = true

[appearance]
size = "medium"                 # "small" (160 px), "medium" (200 px), "large" (240 px)
position = "center"             # "center", "top_left", "top_right", "bottom_left", "bottom_right", "custom"
margin = 24                     # distance from the screen edge, 0–400
always_on_top = true
# custom = [x, y]               # set when you drag the widget

[hooks]                         # shell commands, none by default; see "Hooks" above
# work_start = "dunstctl set-paused true"
```

Values outside the allowed range are clamped. If the file can't be read, it's renamed to `config.toml.broken` and the defaults are used.

## License

Licensed under the [Apache License, Version 2.0](LICENSE). See [NOTICE](NOTICE).

The icons are from [Lucide](https://lucide.dev), licensed under the ISC License (see [`assets/icons/LICENSE`](assets/icons/LICENSE)). The face detection model is UltraFace by linzai, licensed under the MIT License (see [`assets/models/LICENSE`](assets/models/LICENSE)).
