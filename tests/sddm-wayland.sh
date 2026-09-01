#!/usr/bin/env bash
# Verify the installer writes a native Qt Wayland greeter configuration.
set -euo pipefail

repo=${RYOKU_PATH:-$(cd "$(dirname "$0")/.." && pwd)}
setup=$repo/ryoku/lockscreen/sddm/setup

fail() { printf 'sddm-wayland: %s\n' "$*" >&2; exit 1; }

out=$(RYOKU_DRYRUN=1 "$setup" --dry-run)
grep -Fxq 'qt5-wayland' "$repo/system/packages/base.packages" ||
  fail "base package set omits the Qt5 Wayland plugin needed by Qt5 SDDM"
grep -Fq "  'qt5-wayland'" "$repo/release/packages/ryoku-desktop/PKGBUILD" ||
  fail "ryoku-desktop omits the Qt5 Wayland plugin needed by existing systems"
for line in \
  '[General]' \
  'DisplayServer=wayland' \
  'GreeterEnvironment=QT_QPA_PLATFORM=wayland' \
  '[Wayland]' \
  'CompositorCommand='; do

  grep -Fq "$line" <<<"$out" || fail "SDDM setup omits $line"
done

printf 'sddm-wayland: PASS\n'
