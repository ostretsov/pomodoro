#!/bin/bash
# Installs target/pomodoro_*.deb in fresh Ubuntu containers, checks the
# installed files and runs `pomodoro --self-test`.
# Usage: scripts/docker-test.sh [image...]
set -euo pipefail
cd "$(dirname "$0")/.."
images=("${@:-ubuntu:22.04 ubuntu:24.04 ubuntu:rolling}")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp target/pomodoro_*_amd64.deb "$work/pomodoro.deb"
cat > "$work/test.sh" <<'INNER'
#!/bin/bash
set -e
. /etc/os-release; echo "=== $PRETTY_NAME"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null
apt-get install -y -qq --no-install-recommends /work/pomodoro.deb >/tmp/apt.log 2>&1 || { tail -20 /tmp/apt.log; exit 1; }
for f in /usr/bin/pomodoro /etc/xdg/autostart/pomodoro.desktop /usr/share/applications/pomodoro.desktop \
         /usr/share/icons/hicolor/scalable/apps/pomodoro.svg; do
  [ -f "$f" ] || { echo "missing $f"; exit 1; }
done
grep -q '^Exec=pomodoro$' /etc/xdg/autostart/pomodoro.desktop
ldd /usr/bin/pomodoro 2>&1 | grep -qE "not a dynamic executable|statically linked" || { echo "not a static binary:"; ldd /usr/bin/pomodoro; exit 1; }
! dpkg -s pomodoro | grep -q "^Depends:" || { echo "the package has dependencies"; exit 1; }
pomodoro --version
cd /tmp && HOME=/tmp pomodoro --self-test
INNER
chmod +x "$work/test.sh"
status=0
for img in ${images[@]}; do
  docker run --rm -v "$work:/work:ro" "$img" /work/test.sh || status=1
done
exit $status
