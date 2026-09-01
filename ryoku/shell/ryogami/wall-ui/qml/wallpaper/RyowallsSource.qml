import QtQuick
import Quickshell
import Quickshell.Io

// A pluggable remote wallpaper source driven by the ryowalls engine's JSON
// verbs (moewalls, motionbgs, ryostore extras): the same search/download shape
// behind one object, so the picker's Browse surface gains sources without a
// bespoke browser each. Results are NDJSON rows: {id, thumb, video, dl, name,
// resolution, ...}. download() saves the master into the video library and
// emits its path; the picker applies it through the ryogami daemon.
QtObject {
    id: src

    property string searchVerb: ""       // e.g. "moewalls-search"
    property string downloadVerb: ""     // e.g. "moewalls-download"
    property bool needsPost: false       // moewalls download wants the post url

    property var results: []
    property bool loading: false
    property string error: ""

    // id of the row currently downloading, "" when idle
    property string downloadingId: ""
    signal applied(string path)
    signal failed(string reason)

    property string _buf: ""
    property var _searchProc: Process {
        stdout: SplitParser { splitMarker: ""; onRead: function(data) { src._buf += data } }
        onExited: function(code) {
            src.loading = false
            if (code !== 0) { src.error = "search failed"; src.results = []; return }
            var out = []
            var lines = src._buf.split("\n")
            for (var i = 0; i < lines.length; i++) {
                var l = lines[i].trim()
                if (!l) continue
                try { out.push(JSON.parse(l)) } catch (e) {}
            }
            src.error = out.length === 0 ? "no results" : ""
            src.results = out
        }
    }

    function search(query) {
        if (!searchVerb || loading) return
        _buf = ""
        error = ""
        loading = true
        results = []
        var args = [searchVerb]
        if (query && query.length > 0) { args.push("--query"); args.push(query) }
        args.push("--json")
        _searchProc.command = ["ryowalls"].concat(args)
        _searchProc.running = true
    }

    property string _dlBuf: ""
    property var _dlProc: Process {
        stdout: SplitParser { splitMarker: ""; onRead: function(data) { src._dlBuf += data } }
        onExited: function(code) {
            var lines = src._dlBuf.trim().split("\n")
            var path = ""
            for (var i = lines.length - 1; i >= 0; i--) {
                var l = lines[i].trim()
                if (l.length > 0) { path = l; break }
            }
            src.downloadingId = ""
            if (code === 0 && path.length > 0) src.applied(path)
            else src.failed("download failed")
        }
    }

    function download(item) {
        if (!downloadVerb || !item || downloadingId.length > 0) return
        _dlBuf = ""
        downloadingId = "" + item.id
        var args = [downloadVerb, ("" + item.id), ("" + (item.dl || item.video || ""))]
        if (needsPost && item.moewalls_url) args.push("" + item.moewalls_url)
        _dlProc.command = ["ryowalls"].concat(args)
        _dlProc.running = true
    }
}
