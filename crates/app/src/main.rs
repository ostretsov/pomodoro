use std::fs::File;
use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use ksni::blocking::TrayMethods;
use pomodoro::notifier::DesktopNotifier;
use pomodoro::runner::{Runner, Step, XdgAutostart};
use pomodoro::tray::Tray;
use pomodoro::update::GithubUpdates;
use pomodoro::{clock, notifier::Notifier};
use pomodoro_core::autostart::{self, Autostart};
use pomodoro_core::pomodoro::FIRST_UPDATE_CHECK;
use pomodoro_core::{Action, Effect, Phase, Pomodoro, Settings, icon, update};

/// How often the countdown is checked (the menu shows whole seconds).
const TICK: Duration = Duration::from_millis(250);

const HELP: &str = "\
Pomodoro timer in the system tray.

Usage: pomodoro [--minutes N]

Everything else happens in the tray icon's menu: Start, Stop, Reset,
Hide notification, Start at login, Quit.

Options:
  --minutes N   Length of the work period for this run (default: work_minutes
                from ~/.config/pomodoro/settings, which is 20 unless changed)
  --self-test   Check the installation and exit
  --check-update  Ask GitHub whether a newer release exists (the installed
                app checks by itself twice a day and offers it in the menu)
  --version     Print the version
  --help        This text
";

fn main() -> ExitCode {
    let mut minutes = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "--version" | "-V" => {
                println!("pomodoro {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--self-test" => return self_test(),
            "--check-update" => {
                return match pomodoro::update::fetch_newer() {
                    Ok(Some(r)) => {
                        println!("pomodoro {} is available: {}", r.version, r.deb_url);
                        ExitCode::SUCCESS
                    }
                    Ok(None) => {
                        println!(
                            "pomodoro {} is the latest version",
                            env!("CARGO_PKG_VERSION")
                        );
                        ExitCode::SUCCESS
                    }
                    Err(e) => fail(&format!("update check failed: {e:#}")),
                };
            }
            "--minutes" => match args.next().and_then(|m| m.parse::<u32>().ok()) {
                Some(m @ 1..=600) => minutes = Some(m),
                _ => return fail("--minutes needs a whole number from 1 to 600"),
            },
            other => return fail(&format!("unknown argument {other:?} (see --help)")),
        }
    }
    match run(minutes) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("pomodoro: {message}");
    ExitCode::FAILURE
}

fn run(minutes: Option<u32>) -> Result<(), String> {
    let config_home = autostart::config_home(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
    .ok_or("neither XDG_CONFIG_HOME nor HOME is set")?;
    let exec = std::env::current_exe().map_err(|e| format!("own path: {e}"))?;

    let Some(_lock) = single_instance(&config_home)? else {
        eprintln!("pomodoro: already running (see the tray icon)");
        return Ok(());
    };

    let autostart = Autostart {
        config_home: config_home.clone(),
        config_dirs: autostart::config_dirs(std::env::var_os("XDG_CONFIG_DIRS")),
    };
    let mut settings = load_settings(&config_home, &autostart, &exec)?;
    if let Some(m) = minutes {
        settings.work_minutes = m;
    }

    // Keep a user entry pointing at this binary (e.g. after moving it).
    if autostart.has_user_launcher()
        && let Err(e) = autostart.set(true, &exec)
    {
        eprintln!("pomodoro: could not update the autostart entry: {e}");
    }

    let mut app = Pomodoro::new(settings.work(), settings.notice(), autostart.is_enabled());
    // Only an installed package updates itself (not `cargo run`).
    if settings.check_updates && update::is_installed_package(&exec) {
        app = app.with_update_checks(clock::now() + FIRST_UPDATE_CHECK);
    }
    let (tx, rx) = mpsc::channel();
    let mut runner = Runner::new(
        app,
        Box::new(DesktopNotifier::default()),
        Box::new(XdgAutostart { autostart, exec }),
        Box::new(GithubUpdates { tx: tx.clone() }),
        Box::new(clock::now),
    );

    let handle = Tray::new(runner.view(), tx)
        .spawn()
        .map_err(|e| format!("could not create the tray icon: {e}"))?;

    loop {
        let input = match rx.recv_timeout(TICK) {
            Ok(input) => Some(input),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match runner.step(input) {
            Step::Quit => break,
            Step::Continue(Some(view)) => {
                handle.update(|tray| tray.view = view);
            }
            Step::Continue(None) => {}
        }
        if handle.is_closed() {
            return Err("the tray service stopped".into());
        }
    }
    handle.shutdown().wait();
    Ok(())
}

/// Reads `pomodoro/settings`.  On the very first start it writes the
/// defaults and turns on "Start at login".
fn load_settings(
    config_home: &Path,
    autostart: &Autostart,
    exec: &Path,
) -> Result<Settings, String> {
    let path = config_home.join("pomodoro").join("settings");
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let (settings, warnings) =
                Settings::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            for w in warnings {
                eprintln!("pomodoro: {}: {w}", path.display());
            }
            Ok(settings)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let settings = Settings::default();
            let written = std::fs::create_dir_all(path.parent().expect("has a parent"))
                .and_then(|()| std::fs::write(&path, settings.to_file()));
            if let Err(e) = written {
                eprintln!("pomodoro: could not write {}: {e}", path.display());
            }
            if let Err(e) = autostart.set(true, exec) {
                eprintln!("pomodoro: could not turn on start at login: {e}");
            }
            Ok(settings)
        }
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Holds an exclusive lock for the life of the process, so autostart plus a
/// click on the launcher doesn't give two icons.  It lives next to the
/// settings: a test instance with its own XDG_CONFIG_HOME runs alongside.
fn single_instance(config_home: &Path) -> Result<Option<File>, String> {
    let dir = config_home.join("pomodoro");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join("lock");
    let file = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(format!("{}: {e}", path.display())),
    }
}

/// Run after installing (and in CI, in clean containers): exercises
/// everything that doesn't need a desktop session.
fn self_test() -> ExitCode {
    let mut ok = true;
    let mut check = |name: &str, passed: bool| {
        println!("{} {name}", if passed { "ok  " } else { "FAIL" });
        ok &= passed;
    };

    let s = Duration::from_secs;
    let t0 = clock::now();
    let mut p = Pomodoro::new(
        Settings::default().work(),
        Settings::default().notice(),
        false,
    );
    p.handle(Action::Start, t0);
    let finished = p.tick(t0 + s(20 * 60));
    check(
        "a 20 minute period ends with a notification",
        matches!(finished.as_slice(), [Effect::ShowNotice(_)]),
    );
    check(
        "the notification closes after 15 seconds",
        p.tick(t0 + s(20 * 60 + 15)) == [Effect::CloseNotice],
    );
    let px = icon::render(22, Phase::Running, 0.5);
    check(
        "the tray icon renders",
        px.len() == 22 * 22 * 4 && px.iter().any(|&b| b != 0),
    );
    check("the boot clock works", clock::now() >= t0);

    match std::env::var_os("DBUS_SESSION_BUS_ADDRESS") {
        Some(_) => {
            // Shows and closes a real notification: confirms the desktop
            // accepts it.
            let mut n = DesktopNotifier::default();
            n.show(&pomodoro_core::Notice {
                summary: "Pomodoro self-test".into(),
                body: "This notification closes by itself.".into(),
            });
            std::thread::sleep(Duration::from_secs(1));
            n.close();
            println!("ok   sent a test notification");
        }
        None => println!("skip notification (no desktop session)"),
    }

    println!(
        "pomodoro {}: {}",
        env!("CARGO_PKG_VERSION"),
        if ok {
            "self-test passed"
        } else {
            "self-test FAILED"
        }
    );
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
