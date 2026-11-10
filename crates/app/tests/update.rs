//! The update worker against a local HTTP server standing in for GitHub:
//! the release check, the download and its SHA-256 check.  (Installing needs
//! pkexec and a real package; `update::install_command` is unit-tested.)

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use pomodoro::update::{download, fetch_newer};
use pomodoro_core::update::{Release, URL_ENV, Version};
use sha2::{Digest, Sha256};

/// Serves `routes` (path → body) on 127.0.0.1; returns the base URL.
fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            let (status, body) = match routes.iter().find(|(p, _)| *p == path) {
                Some((_, body)) => ("200 OK", body.clone()),
                None => ("404 Not Found", b"not found".to_vec()),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    base
}

fn hex_sha256(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pomodoro-update-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn check_and_download() {
    let deb = vec![7u8; 300_000];
    let latest = format!(
        r#"{{"tag_name":"v99.0.0","html_url":"https://github.com/ostretsov/pomodoro/releases/tag/v99.0.0",
            "assets":[{{"name":"pomodoro_99.0.0_amd64.deb","size":{},"digest":"sha256:{}",
            "browser_download_url":"https://github.com/ostretsov/pomodoro/releases/download/v99.0.0/pomodoro_99.0.0_amd64.deb"}}]}}"#,
        deb.len(),
        hex_sha256(&deb)
    );
    let base = serve(vec![
        ("/latest", latest.into_bytes()),
        ("/deb", deb.clone()),
    ]);

    // SAFETY: the only test in this binary.
    unsafe { std::env::set_var(URL_ENV, format!("{base}/latest")) };
    let found = fetch_newer().unwrap().expect("99.0.0 is newer");
    assert_eq!(found.version, Version(99, 0, 0));
    assert_eq!(found.deb_size, deb.len() as u64);

    // Download it from the local server instead of GitHub.
    let release = Release {
        deb_url: format!("{base}/deb"),
        ..found
    };
    let dir = scratch("ok");
    let mut progress = Vec::new();
    let path = download(&release, &dir, |done| progress.push(done)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), deb);
    assert_eq!(path.file_name().unwrap(), "pomodoro_99.0.0_amd64.deb");
    assert_eq!(progress.last(), Some(&(deb.len() as u64)));

    // A damaged download is refused.
    let damaged = Release {
        sha256: Some(hex_sha256(b"something else")),
        ..release.clone()
    };
    let err = download(&damaged, &scratch("bad-sha"), |_| {}).unwrap_err();
    assert!(err.to_string().contains("SHA-256"), "{err}");

    // So is a short one.
    let short = Release {
        deb_size: deb.len() as u64 + 1,
        ..release.clone()
    };
    let err = download(&short, &scratch("short"), |_| {}).unwrap_err();
    assert!(err.to_string().contains("incomplete"), "{err}");

    // Missing file.
    let missing = Release {
        deb_url: format!("{base}/nothing"),
        ..release
    };
    assert!(download(&missing, &scratch("missing"), |_| {}).is_err());

    // GitHub unreachable or no release yet.
    unsafe { std::env::set_var(URL_ENV, format!("{base}/nothing")) };
    assert!(
        fetch_newer().is_err(),
        "404 is an error (logged, not shown)"
    );
}
