//! Updates from GitHub releases, done in background threads: look for a newer
//! release, download its `.deb`, check the SHA-256 GitHub reports and
//! install it with `pkexec apt-get` (Ubuntu shows its password dialog).
//! Progress goes back to the main loop as [`update::Event`]s; the logic and
//! the menu texts are in `pomodoro_core::update`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{Context, bail};
use pomodoro_core::update::{self, Event, Release};
use sha2::{Digest, Sha256};

use crate::runner::{Input, Updates};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn user_agent() -> String {
    format!("pomodoro/{VERSION} (+https://github.com/{})", update::REPO)
}

/// Asks GitHub for the latest release (blocking).
pub fn fetch_newer() -> anyhow::Result<Option<Release>> {
    let url = std::env::var(update::URL_ENV).unwrap_or_else(|_| update::LATEST_URL.into());
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .new_agent();
    let json = agent
        .get(&url)
        .header("User-Agent", user_agent())
        .header("Accept", "application/vnd.github+json")
        .call()
        .with_context(|| format!("cannot reach {url}"))?
        .body_mut()
        .read_to_string()?;
    update::newer_release(VERSION, &json)
}

/// A new directory only this run uses, readable by apt's `_apt` user.
fn fresh_dir() -> anyhow::Result<PathBuf> {
    for _ in 0..10 {
        let dir = std::env::temp_dir().join(format!(
            "pomodoro-update-{}-{:08x}",
            std::process::id(),
            rand_u32()
        ));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).context(dir.display().to_string()),
        }
    }
    bail!(
        "cannot create a download folder in {}",
        std::env::temp_dir().display()
    )
}

fn rand_u32() -> u32 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    t.subsec_nanos() ^ (t.as_secs() as u32).rotate_left(16)
}

/// Downloads the release's `.deb` into `dir` and checks its SHA-256;
/// `progress` gets the bytes received so far.
pub fn download(r: &Release, dir: &Path, mut progress: impl FnMut(u64)) -> anyhow::Result<PathBuf> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15 * 60)))
        .build()
        .new_agent();
    let mut resp = agent
        .get(&r.deb_url)
        .header("User-Agent", user_agent())
        .call()
        .with_context(|| format!("cannot download {}", r.deb_url))?;
    let path = dir.join(&r.deb_name);
    let mut file = std::fs::File::create(&path).context(path.display().to_string())?;
    let mut reader = resp.body_mut().as_reader();
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).context("download interrupted")?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        done += n as u64;
        progress(done);
    }
    file.sync_all()?;
    if r.deb_size > 0 && done != r.deb_size {
        bail!("download incomplete ({done} of {} bytes)", r.deb_size);
    }
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    match &r.sha256 {
        Some(want) if *want != got => bail!("the download is damaged (SHA-256 mismatch)"),
        Some(_) => {}
        None => eprintln!("pomodoro: update: GitHub reports no SHA-256 for the package"),
    }
    Ok(path)
}

/// Installs `deb` with `pkexec apt-get` (blocking; shows the password dialog).
pub fn install(deb: &Path) -> anyhow::Result<()> {
    let cmd = update::install_command(deb);
    let out = std::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("cannot run {}", cmd[0]))?;
    if let Some(why) = update::install_failure(out.status.code()) {
        let detail = String::from_utf8_lossy(&out.stderr)
            .lines()
            .rfind(|l| l.starts_with("E:"))
            .map(|l| format!(": {}", l.trim_start_matches("E:").trim()))
            .unwrap_or_default();
        bail!("{why}{detail}");
    }
    Ok(())
}

/// Starts the (new) pomodoro with the same arguments once this process ended
/// (it holds the single-instance lock until then).
pub fn spawn_restarter() -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    std::process::Command::new("sh")
        .arg("-c")
        .arg(update::restart_script(std::process::id()))
        .arg("pomodoro")
        .args(std::env::args().skip(1))
        .stdin(std::process::Stdio::null())
        .process_group(0)
        .spawn()?;
    Ok(())
}

/// The real [`Updates`]: GitHub, pkexec, `sh` for the restart.
pub struct GithubUpdates {
    pub tx: Sender<Input>,
}

impl GithubUpdates {
    fn send(tx: &Sender<Input>, event: Event) {
        let _ = tx.send(Input::Update(event));
    }
}

impl Updates for GithubUpdates {
    fn check(&mut self) {
        let tx = self.tx.clone();
        std::thread::spawn(move || match fetch_newer() {
            Ok(Some(r)) => Self::send(&tx, Event::Found(r)),
            Ok(None) => {}
            // Offline or GitHub unreachable: not worth bothering the user.
            Err(e) => eprintln!("pomodoro: update check failed: {e:#}"),
        });
    }

    fn install(&mut self, r: Release) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let dir = fresh_dir()?;
                let mut last = u8::MAX;
                let deb = download(&r, &dir, |done| {
                    let pct = (done * 100 / r.deb_size.max(1)).min(100) as u8;
                    if pct != last {
                        last = pct;
                        Self::send(&tx, Event::Progress(pct));
                    }
                });
                let res = deb.and_then(|deb| {
                    Self::send(&tx, Event::Installing);
                    install(&deb)
                });
                let _ = std::fs::remove_dir_all(&dir);
                res
            })();
            Self::send(
                &tx,
                match result {
                    Ok(()) => Event::Installed,
                    Err(e) => Event::Failed(format!("{e:#}")),
                },
            );
        });
    }

    fn restart(&mut self) -> anyhow::Result<()> {
        spawn_restarter()
    }
}
