//! "Start at login": XDG autostart entries.  GNOME (and every other
//! freedesktop session) starts what is in `~/.config/autostart` and in
//! `/etc/xdg/autostart` after login; a user entry with the same file name
//! overrides the system one.
//!
//! The .deb installs the system entry, so the timer starts at the next login
//! without opening it once.  Turning it off then writes a user entry with
//! `Hidden=true`.  Without a system entry (e.g. a binary from `cargo build`)
//! the user entry itself runs the app.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "pomodoro.desktop";

/// `$XDG_CONFIG_HOME`, or `~/.config`.
pub fn config_home(xdg_config_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    match xdg_config_home {
        Some(dir) if Path::new(&dir).is_absolute() => Some(dir.into()),
        _ => home.map(|h| PathBuf::from(h).join(".config")),
    }
}

/// `$XDG_CONFIG_DIRS`, or `/etc/xdg`.
pub fn config_dirs(xdg_config_dirs: Option<OsString>) -> Vec<PathBuf> {
    let dirs: Vec<PathBuf> = xdg_config_dirs
        .map(|v| {
            std::env::split_paths(&v)
                .filter(|p| p.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    if dirs.is_empty() {
        vec![PathBuf::from("/etc/xdg")]
    } else {
        dirs
    }
}

#[derive(Debug, Clone)]
pub struct Autostart {
    pub config_home: PathBuf,
    pub config_dirs: Vec<PathBuf>,
}

impl Autostart {
    pub fn user_entry(&self) -> PathBuf {
        self.config_home.join("autostart").join(FILE_NAME)
    }

    fn system_entry(&self) -> Option<PathBuf> {
        self.config_dirs
            .iter()
            .map(|d| d.join("autostart").join(FILE_NAME))
            .find(|p| p.is_file())
    }

    /// Whether the session will start the app at the next login.
    pub fn is_enabled(&self) -> bool {
        match std::fs::read_to_string(self.user_entry()) {
            Ok(text) => entry_enabled(&text),
            Err(_) => self
                .system_entry()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .is_some_and(|text| entry_enabled(&text)),
        }
    }

    /// Whether there is a user entry that runs the app (not just one hiding
    /// the system entry).
    pub fn has_user_launcher(&self) -> bool {
        std::fs::read_to_string(self.user_entry()).is_ok_and(|t| entry_enabled(&t))
    }

    pub fn set(&self, enabled: bool, exec: &Path) -> io::Result<()> {
        let path = self.user_entry();
        let system = self.system_entry().is_some();
        let content = match (enabled, system) {
            (true, true) | (false, false) => None,
            (true, false) => Some(desktop_entry(exec)),
            (false, true) => Some(hidden_entry()),
        };
        match content {
            Some(text) => {
                std::fs::create_dir_all(path.parent().expect("has a parent"))?;
                std::fs::write(&path, text)
            }
            None => match std::fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            },
        }
    }
}

/// `Hidden=true` deletes an entry; `X-GNOME-Autostart-enabled=false` is what
/// GNOME's "Startup Applications" writes.
fn entry_enabled(text: &str) -> bool {
    !text.lines().map(|l| l.replace(' ', "")).any(|l| {
        l.eq_ignore_ascii_case("Hidden=true")
            || l.eq_ignore_ascii_case("X-GNOME-Autostart-enabled=false")
    })
}

fn hidden_entry() -> String {
    "[Desktop Entry]\n\
     Type=Application\n\
     Name=Pomodoro\n\
     Exec=pomodoro\n\
     Hidden=true\n"
        .to_string()
}

pub fn desktop_entry(exec: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Pomodoro\n\
         Comment=Pomodoro timer in the system tray\n\
         Exec={}\n\
         Icon=pomodoro\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n",
        quote_exec(&exec.to_string_lossy())
    )
}

/// Quoting for the `Exec` key (Desktop Entry Specification).
fn quote_exec(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_alphanumeric() || "/._-+,:@".contains(c));
    if plain {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    for c in arg.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                out.push_str("\\\\");
                out.push(c);
            }
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pomodoro-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn config_home_prefers_absolute_xdg() {
        assert_eq!(
            config_home(Some("/x/cfg".into()), Some("/home/a".into())),
            Some(PathBuf::from("/x/cfg"))
        );
        assert_eq!(
            config_home(Some("relative".into()), Some("/home/a".into())),
            Some(PathBuf::from("/home/a/.config"))
        );
        assert_eq!(
            config_home(None, Some("/home/a".into())),
            Some(PathBuf::from("/home/a/.config"))
        );
        assert_eq!(config_home(None, None), None);
    }

    #[test]
    fn entry_runs_the_given_binary() {
        let e = desktop_entry(Path::new("/usr/bin/pomodoro"));
        assert!(e.starts_with("[Desktop Entry]\n"));
        assert!(e.contains("\nExec=/usr/bin/pomodoro\n"));
        assert!(e.contains("\nType=Application\n"));
    }

    #[test]
    fn exec_is_quoted_when_needed() {
        assert_eq!(quote_exec("/opt/my apps/pomo"), "\"/opt/my apps/pomo\"");
        assert_eq!(quote_exec("/a/$x"), "\"/a/\\\\$x\"");
        assert_eq!(quote_exec("/a/100%"), "\"/a/100%%\"");
    }

    fn setup(name: &str, system_entry: Option<&str>) -> Autostart {
        let dir = scratch(name);
        let a = Autostart {
            config_home: dir.join("home"),
            config_dirs: vec![dir.join("none"), dir.join("etc")],
        };
        if let Some(text) = system_entry {
            std::fs::create_dir_all(dir.join("etc/autostart")).unwrap();
            std::fs::write(dir.join("etc/autostart").join(FILE_NAME), text).unwrap();
        }
        a
    }

    const EXEC: &str = "/home/a/bin/pomodoro";

    #[test]
    fn without_system_entry_the_user_entry_runs_the_app() {
        let a = setup("user-only", None);
        assert!(!a.is_enabled());
        a.set(true, Path::new(EXEC)).unwrap();
        assert!(a.is_enabled() && a.has_user_launcher());
        let text = std::fs::read_to_string(a.user_entry()).unwrap();
        assert!(text.contains("Exec=/home/a/bin/pomodoro\n"));
        a.set(false, Path::new(EXEC)).unwrap();
        assert!(!a.is_enabled());
        assert!(!a.user_entry().exists());
        a.set(false, Path::new(EXEC)).unwrap();
    }

    #[test]
    fn system_entry_is_hidden_by_a_user_entry() {
        let a = setup(
            "system",
            Some(&desktop_entry(Path::new("/usr/bin/pomodoro"))),
        );
        assert!(a.is_enabled(), "installed package: on by default");
        assert!(!a.has_user_launcher());
        a.set(false, Path::new(EXEC)).unwrap();
        assert!(!a.is_enabled());
        assert!(
            std::fs::read_to_string(a.user_entry())
                .unwrap()
                .contains("Hidden=true")
        );
        assert!(!a.has_user_launcher());
        a.set(true, Path::new(EXEC)).unwrap();
        assert!(a.is_enabled());
        assert!(!a.user_entry().exists(), "the system entry starts it");
    }

    #[test]
    fn gnome_startup_applications_switch_is_respected() {
        let a = setup(
            "gnome-off",
            Some(&desktop_entry(Path::new("/usr/bin/pomodoro"))),
        );
        std::fs::create_dir_all(a.user_entry().parent().unwrap()).unwrap();
        std::fs::write(
            a.user_entry(),
            "[Desktop Entry]\nX-GNOME-Autostart-enabled=false\n",
        )
        .unwrap();
        assert!(!a.is_enabled());
    }

    #[test]
    fn config_dirs_default_to_etc_xdg() {
        assert_eq!(config_dirs(None), [PathBuf::from("/etc/xdg")]);
        assert_eq!(config_dirs(Some("".into())), [PathBuf::from("/etc/xdg")]);
        assert_eq!(
            config_dirs(Some("/a:rel:/b".into())),
            [PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }
}
