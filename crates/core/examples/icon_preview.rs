//! Writes the tray icon in every state as PAM images (view or convert with
//! ImageMagick): `cargo run -p pomodoro-core --example icon_preview -- DIR`.

use pomodoro_core::{Phase, icon};

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let states = [
        ("idle", Phase::Idle, 1.0),
        ("running-75", Phase::Running, 0.75),
        ("running-30", Phase::Running, 0.3),
        ("paused-50", Phase::Paused, 0.5),
        ("finished", Phase::Finished, 0.0),
    ];
    for size in [22u32, 48] {
        for (name, phase, fraction) in states {
            let argb = icon::render(size, phase, fraction);
            let mut out = format!(
                "P7\nWIDTH {size}\nHEIGHT {size}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n"
            )
            .into_bytes();
            for px in argb.chunks(4) {
                out.extend_from_slice(&[px[1], px[2], px[3], px[0]]);
            }
            let path = format!("{dir}/{name}-{size}.pam");
            std::fs::write(&path, out).unwrap();
            println!("{path}");
        }
    }
}
