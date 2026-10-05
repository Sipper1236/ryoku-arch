#!/usr/bin/env bash
set -euo pipefail

config_root="${VESKTOP_CONFIG_ROOT:-$HOME/.config/vesktop}"
template="${MATUGEN_VESKTOP_TEMPLATE:-$HOME/.config/matugen/templates/vesktop-colors.css}"
settings="$config_root/settings/settings.json"
quick_css="$config_root/settings/quickCss.css"

if rg -q '^@import .*midnight' "$template"; then
  printf 'FAIL: Matugen rewrites the Midnight import on every palette change\n' >&2
  exit 1
fi

jq -e '.useQuickCss == true' "$settings" >/dev/null
ryoku_theme=$(jq -r '(.enabledThemes // [])[] | select(ascii_downcase == "ryoku.theme.css")' "$settings" | head -n1)
if [[ -n "$ryoku_theme" ]]; then
  base="$config_root/themes/$ryoku_theme"
  test -f "$base"
  rg -q -- '--ryo-palette-mode: var\(--ryo-bridge-enabled, 0\)' "$base"
  rg -q -- 'var\(--accent-2' "$base"
  jq -e '(.enabledThemes // []) | index("midnight-ryoku.theme.css") == null' "$settings" >/dev/null
  base_name=Ryoku
else
  base="$config_root/themes/midnight-ryoku.theme.css"
  test -f "$base"
  rg -q '^@import .*midnight' "$base"
  jq -e '(.enabledThemes // []) | index("midnight-ryoku.theme.css") != null' "$settings" >/dev/null
  base_name=Midnight
fi
rg -q -- '--ryo-bridge-enabled: 1;' "$quick_css"
rg -q -- '--accent-2:' "$quick_css"
printf 'PASS: stable %s base and isolated QuickCSS palette\n' "$base_name"
