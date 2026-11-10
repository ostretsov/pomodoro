# Pomodoro

A small Pomodoro timer for Ubuntu that lives in the system tray (Rust).

- Starts at login and shows a tomato-timer icon in the top bar: a rounded
  tile with a white dial whose dark pie is the time left, shrinking clockwise
  like a kitchen timer. Red while working, amber when stopped, green with a
  check mark when the time is up.
- **Start** runs a 20-minute work period (the default); the first line of
  the menu shows the exact time left.
- When the time runs out, a "Time's up!" notification stays on screen for
  15 seconds.
- The icon's menu (left or right click):

  | Item | What it does |
  |---|---|
  | Start (20 min) / Resume (mm:ss) | Start a new period, or continue a stopped one |
  | Stop | Pause the countdown |
  | Reset | Back to 20:00, not running |
  | Hide notification | Close the "Time's up!" notification now |
  | Install update X.Y.Z… | Only when a newer release exists (see Updates) |
  | Start at login | Turn autostart on or off |
  | Quit | Exit (closes the notification too) |

Suspend doesn't freeze the timer: a period that ends while the laptop sleeps
shows "Time's up!" right after it wakes up.

## Install

Download `pomodoro_0.1.0_amd64.deb` from the
[releases](https://github.com/ostretsov/pomodoro/releases), then:

```sh
sudo apt install ./pomodoro_0.1.0_amd64.deb
pomodoro &        # or open "Pomodoro" from the app grid; after that it starts at login
```

The package is self-contained: one statically linked binary (D-Bus, TLS
and its certificates built in), no dependencies, so it installs on any
Ubuntu (tested on clean 22.04, 24.04 and the latest release). The tray icon needs the "Ubuntu AppIndicators" GNOME
extension, which Ubuntu has enabled by default (`gnome-extensions list --enabled`
shows `ubuntu-appindicators@ubuntu.com`). On other desktops any
StatusNotifierItem tray works (KDE, XFCE with the SNI plugin, …).

## Updates

The installed app asks GitHub for the latest release 30 seconds after it
starts and then twice a day. When there is a newer one, a notification says
"Pomodoro X.Y.Z is available" and the menu gets **Install update X.Y.Z (size)…**:
click it and it downloads the `.deb`, checks its SHA-256 against the one
GitHub reports and installs it with `apt` (Ubuntu asks for your password).
Then **Restart to use X.Y.Z** starts the new version. Offline, nothing is
shown. `check_updates = false` in the settings turns this off;
`pomodoro --check-update` asks right away.

## Settings

`~/.config/pomodoro/settings` is created on the first start:

```ini
work_minutes = 20     # length of a work period
notice_seconds = 15   # how long "Time's up!" stays on screen
check_updates = true  # look for new releases on GitHub (installed package only)
```

Quit and start the app again after editing. `pomodoro --minutes 25` overrides
the length for one run. `pomodoro --self-test` checks an installation.

Start at login: the package installs `/etc/xdg/autostart/pomodoro.desktop`;
switching "Start at login" off writes `~/.config/autostart/pomodoro.desktop`
with `Hidden=true` (GNOME's "Startup Applications" setting is respected too).

## Build from source

```sh
make test        # unit tests + an end-to-end D-Bus test on a private bus
make demo        # a 1-minute timer with scratch settings in target/demo
make deb         # target/pomodoro_0.1.0_amd64.deb (static musl binary)
make docker-test # install the .deb in clean Ubuntu 22.04/24.04/rolling containers
make help        # everything else
```

Needs Rust (https://rustup.rs) and `dbus-daemon` (package `dbus`, installed on
every Ubuntu desktop) for the tests. No `-dev` packages: D-Bus is spoken in
pure Rust ([ksni](https://crates.io/crates/ksni) for the tray,
[notify-rust](https://crates.io/crates/notify-rust) for notifications), HTTPS
with [ureq](https://crates.io/crates/ureq)/rustls. `make deb` adds the
`x86_64-unknown-linux-musl` target with rustup if needed; it uses `musl-gcc`
(`apt install musl-tools`) when present, else the system `gcc`.
`cargo run -p pomodoro-core --example icon_preview -- DIR` writes the tray
icon in every state as images.

## Releases

Pushing a `vX.Y.Z` tag runs `.github/workflows/release.yml`: lint, tests,
`.deb`, Docker self-tests, then a GitHub release with the `.deb` attached.
With Claude Code, `/release [X.Y.Z|major|minor|patch]` does the bump, commit,
tag and push (`.claude/skills/release/SKILL.md`). By hand:

```sh
make version V=0.2.0 && git commit -am "Release 0.2.0" && git tag -a v0.2.0 -m "Pomodoro 0.2.0" && git push origin main --follow-tags
```
