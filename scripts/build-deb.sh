#!/bin/bash
# Builds target/pomodoro_<version>_amd64.deb.  Install it with
#   sudo apt install ./target/pomodoro_*.deb
#
# Self-contained: one statically linked (musl) binary with everything inside
# (D-Bus, TLS and its root certificates are pure Rust), so the package
# depends on nothing and runs on any x86-64 Linux.  The tray icon needs the
# AppIndicator GNOME extension, which Ubuntu ships enabled.
set -euo pipefail
cd "$(dirname "$0")/.."
target=x86_64-unknown-linux-musl
rustup target list --installed 2>/dev/null | grep -qx $target || rustup target add $target
# ring (TLS) compiles some C: musl-gcc if installed (apt install musl-tools),
# otherwise the system gcc works too.
if [ -z "${CC_x86_64_unknown_linux_musl:-}" ]; then
  export CC_x86_64_unknown_linux_musl=$(command -v musl-gcc || command -v x86_64-linux-musl-gcc || echo gcc)
fi
cargo build --release --locked --target $target
bin=target/$target/release/pomodoro
version=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/app/Cargo.toml | head -1)
root=target/deb/pomodoro_${version}_amd64
rm -rf "$root"
install -Dm755 "$bin" "$root/usr/bin/pomodoro"
install -Dm644 packaging/pomodoro.desktop "$root/usr/share/applications/pomodoro.desktop"
install -Dm644 packaging/pomodoro-autostart.desktop "$root/etc/xdg/autostart/pomodoro.desktop"
install -Dm644 packaging/pomodoro.svg "$root/usr/share/icons/hicolor/scalable/apps/pomodoro.svg"
install -Dm644 README.md "$root/usr/share/doc/pomodoro/README.md"
install -Dm644 LICENSE "$root/usr/share/doc/pomodoro/copyright"

# Self-contained means no shared libraries at all.
needed=$(objdump -p "$bin" | awk '/NEEDED/ {print $2}')
[ -z "$needed" ] || { echo "The binary links shared libraries: $needed" >&2; exit 1; }
"$bin" --version >/dev/null

mkdir -p "$root/DEBIAN"
cat > "$root/DEBIAN/control" <<CONTROL
Package: pomodoro
Version: $version
Architecture: amd64
Maintainer: Artem Ostretsov <ostretsovaa@gmail.com>
Section: utils
Priority: optional
Recommends: gnome-shell-extension-appindicator
Installed-Size: $(du -sk "$root" | cut -f1)
Description: Pomodoro timer in the system tray
 Starts at login and sits in the tray.  Start a 20 minute work period from
 the icon's menu; when it runs out a notification stays on screen for 15
 seconds.  Stop, reset or hide the notification from the same menu.
CONTROL
dpkg-deb --root-owner-group --build "$root" "target/pomodoro_${version}_amd64.deb"
echo "Built target/pomodoro_${version}_amd64.deb"
