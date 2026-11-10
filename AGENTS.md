# Notes for coding agents

Pomodoro: a tray timer for Ubuntu (Rust). `README.md` describes the features;
this file is about working on the code.

## Layout

- `crates/core` — everything testable without a desktop: the timer state
  machine (`timer.rs`), what menu actions do and what the tray shows
  (`pomodoro.rs`: `Action` in, `Effect`s out, `View`), the tray icon pixmap
  (`icon.rs`), settings, autostart entries, update logic (`update.rs`: GitHub
  JSON, versions, the update menu item's states). Put new logic here, with
  tests.
  Time is a `Duration` passed in, never read inside core.
- `crates/app` — the desktop side: `tray.rs` (StatusNotifierItem + dbusmenu
  via `ksni`), `notifier.rs` (notify-rust), `runner.rs` (executes `Effect`s,
  pushes changed `View`s to the tray), `clock.rs` (CLOCK_BOOTTIME, so suspend
  counts), `update.rs` (background check/download/SHA-256/pkexec install,
  restart), `main.rs` (args, settings, single-instance lock, the loop).
  Menu clicks and update progress both reach the loop as `runner::Input`.
- `crates/app/tests/dbus.rs` starts a private `dbus-daemon` with a fake tray
  host and a fake notification daemon and drives the real tray/notifier over
  D-Bus. `tests/update.rs` serves a fake GitHub release on localhost. Keep
  each the only test in its file (they set the process environment).

## Build and test

- `make help` lists everything. `make test` and `make lint` (fmt + clippy
  with `-D warnings`) must pass before you finish.
- No system `-dev` packages are needed; keep it that way (pure-Rust D-Bus
  and TLS). The .deb is self-contained: `scripts/build-deb.sh` builds a static
  musl binary and fails if it links any shared library, and the package has
  no `Depends`.
- Updates: only an installed `/usr/bin/pomodoro` checks GitHub. Don't hit
  the GitHub API in loops: unauthenticated it allows 60 requests an hour per
  IP (this machine has run out before; a 403 then is not a bug).

## Running it on the user's desktop

- The user's real instance may be running. A test instance needs its own
  `XDG_CONFIG_HOME` (`make demo` uses `target/demo`): the single-instance lock,
  the settings and the autostart entry all live there. Never run a dev build
  with the real `~/.config`: its first start would point autostart at
  `target/`.
- Start background instances with `setsid … & disown`. Stop them with
  `pkill -x pomodoro` only if you're sure the user's own instance isn't
  running; otherwise kill your PID.
- Look at the tray/menu over D-Bus (`busctl --user`, as in `tests/dbus.rs`),
  not with fake mouse input.

## Notifications on GNOME

GNOME ignores `expire_timeout` and hides normal notifications after ~4 s. So
the notification is sent with urgency *critical* (stays until closed) and
`Timeout::Never`, and the app closes it itself after `notice_seconds` (or on
"Hide notification", Start, Reset, Quit).

## Packaging and releases

- `make deb` → `target/pomodoro_<version>_amd64.deb` (binary, app-grid entry,
  `/etc/xdg/autostart` entry, SVG icon). `make docker-test` installs it in
  clean Ubuntu containers and runs `--self-test`; run it after packaging
  changes.
- Releasing: the `/release` skill (`.claude/skills/release/SKILL.md`) bumps
  the version, commits, tags `vX.Y.Z` and pushes; the tag's CI run
  (`.github/workflows/release.yml`) publishes the `.deb`, which installed
  apps then offer as an update. A broken release reaches users that way.
