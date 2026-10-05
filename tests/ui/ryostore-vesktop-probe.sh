#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$here/../.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/Ryoku" "$work/bin" "$work/state" "$work/runtime"
chmod 700 "$work/runtime"
ln -s "$repo/ryoku/ui" "$work/Ryoku/Ui"
cp -a "$repo/ryoku/apps/ryostore/quickshell" "$work/ryostore"
cp "$here/ryostore-vesktop-probe.qml" "$work/probe.qml"
cat >"$work/bin/ryostore" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
state="${RYOSTORE_FIXTURE_STATE:?}"
case "${1:-}" in
catalog)
    python3 - "$state" <<'PYTHON'
import json, pathlib, sys
state=pathlib.Path(sys.argv[1])
items=[]
for name, category, provider, installed in [('discord','vesktop-themes','Shared',False),('second','vesktop-themes','Shared',False),('scheme','colorschemes','Shared',False),('old','vesktop-themes','Shared',True),('paused','vesktop-themes','Shared',False),('unavailable','vesktop-themes','Shared',False)]:
    items.append(dict(id=name,category=category,name=name,metadata={'provider':provider},installed=installed or (state/name).exists(),active=False,enabled=False,downloadPaused=name=='paused',unavailable=name=='unavailable'))
print(json.dumps({'categories':[{'id':'vesktop-themes','name':'Vesktop themes','group':'wear','count':5},{'id':'colorschemes','name':'Themes','group':'wear','count':1}],'items':items}))
PYTHON
    ;;
install)
    [[ ${2:-} == vesktop-themes && ( ${3:-} == discord || ${3:-} == second ) ]] || exit 2
    printf '%s\n' "$*" >>"$state/commands"
    : >"$state/$3"
    ;;
check) echo '{}' ;;
warm) : ;;
*) printf 'Unexpected command: %s\n' "$*" >&2; exit 2 ;;
esac
FAKE
chmod +x "$work/bin/ryostore"
QT_QPA_PLATFORM=offscreen \
XDG_RUNTIME_DIR="$work/runtime" \
XDG_CONFIG_HOME="$work/config" \
XDG_CACHE_HOME="$work/cache" \
PATH="$work/bin:$PATH" \
RYOSTORE_FIXTURE_STATE="$work/state" \
QML2_IMPORT_PATH="$work:${QML2_IMPORT_PATH:-$HOME/.local/lib/qt6/qml}" \
    timeout 25 qs -p "$work/probe.qml" >"$work/log" 2>&1 || true
if ! rg -q VESKTOP-UI-PASS "$work/log" || rg -q ' ERROR|TypeError|ReferenceError|VESKTOP-UI-FAIL' "$work/log"; then
    cat "$work/log"
    exit 1
fi
printf '%s\n' 'install vesktop-themes discord' 'install vesktop-themes second' >"$work/expected"
diff -u "$work/expected" "$work/state/commands"
echo 'ryostore-vesktop-probe: scoped bulk install, skipped items, guidance and navigation'
