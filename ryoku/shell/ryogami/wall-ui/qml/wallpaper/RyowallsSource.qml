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
    property var extraArgs: []            // e.g. ["--repo", "owner/repo"] for library-list

    // native (no-binary) source paths, phased in per provider; the ryowalls
    // binary stays the fallback until every provider is ported (then it sunsets).
    readonly property string _ua: "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0"
    readonly property string _ryostoreBase: "https://raw.githubusercontent.com/neur0map/ryostore/main"
    readonly property string _mbBase: "https://motionbgs.com"
    readonly property string _mwBase: "https://moewalls.com"
    property string _nativeProvider: ""
    property string _pendingQuery: ""
    property string _nativeDlBuf: ""

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
        if (searchVerb === "extras-search") {
            _nativeProvider = "ryostore"
            _pendingQuery = query || ""
            _nativeSearchProc.command = ["curl", "-fsSL", "-A", _ua, _ryostoreBase + "/livewalls/registry.json"]
            _nativeSearchProc.running = true
            return
        }
        if (searchVerb === "motionbgs-search") {
            _nativeProvider = "motionbgs"
            var q2 = ("" + (query || "")).toLowerCase().replace(/ /g, "-")
            var mbPath = q2.length > 0 ? "/tag:" + q2 + "/" : "/"
            _nativeSearchProc.command = ["curl", "-fsSL", "-A", _ua, "-e", _mbBase + "/", _mbBase + mbPath]
            _nativeSearchProc.running = true
            return
        }
        if (searchVerb === "moewalls-search") {
            _nativeProvider = "moewalls"
            var mwUrl = (query && query.length > 0)
                ? _mwBase + "/?s=" + encodeURIComponent(query)
                : _mwBase + "/anime/"
            _nativeSearchProc.command = ["curl", "-fsSL", "-A", _ua, "-e", _mwBase + "/", mwUrl]
            _nativeSearchProc.running = true
            return
        }
        var args = [searchVerb]
        for (var e = 0; e < extraArgs.length; e++) args.push(extraArgs[e])
        if (query && query.length > 0) { args.push("--query"); args.push(query) }
        args.push("--json")
        _searchProc.command = ["ryowalls"].concat(args)
        _searchProc.running = true
    }

    // native ryostore: fetch the curated registry.json and reshape it into the
    // same rows the binary emitted, so the Browse surface is unchanged.
    property var _nativeSearchProc: Process {
        stdout: SplitParser { splitMarker: ""; onRead: function(data) { src._buf += data } }
        onExited: function(code) {
            src.loading = false
            if (code !== 0) { src.error = "search failed"; src.results = []; return }
            var out = []
            try {
                if (src._nativeProvider === "ryostore") {
                    var reg = JSON.parse(src._buf)
                    var q = ("" + src._pendingQuery).toLowerCase()
                    var ws = reg.wallpapers || []
                    for (var i = 0; i < ws.length; i++) {
                        var w = ws[i]
                        var hit = q === "" || ("" + (w.name || "")).toLowerCase().indexOf(q) >= 0
                        var tags = w.tags || []
                        for (var t = 0; !hit && t < tags.length; t++)
                            if (("" + tags[t]).toLowerCase().indexOf(q) >= 0) hit = true
                        if (!hit) continue
                        out.push({ id: w.id, thumb: src._ryostoreBase + "/" + w.poster,
                                   video: w.video, dl: w.video, resolution: "",
                                   name: w.name, author: (w.author || "") })
                    }
                }
                else if (src._nativeProvider === "motionbgs") {
                    var re = new RegExp("/i/c/[0-9]+x[0-9]+/media/[0-9]+/[^\"' ]+\\.jpe?g", "g")
                    var seen = {}, m
                    while ((m = re.exec(src._buf)) !== null) {
                        var p = m[0]
                        var parts = p.split("/")           // ['','i','c',WxH,'media',id,fname]
                        var wxh = parts[3], mid = parts[5], fname = parts[6]
                        if (parseInt(wxh.split("x")[0]) < 300) continue
                        if (seen[mid]) continue
                        seen[mid] = true
                        var mbase = ("" + fname).replace(/\.[0-9]+x[0-9]+/, "").replace(/\.jpe?g$/, "")
                        var rmt = ("" + fname).match(/\.([0-9]+x[0-9]+)\.jpe?g$/)
                        out.push({ id: mid, thumb: src._mbBase + p,
                                   video: src._mbBase + "/dl/hd/" + mid + "/",
                                   dl: src._mbBase + "/dl/4k/" + mid + "/",
                                   resolution: rmt ? rmt[1] : "", name: mbase.replace(/-/g, " ") })
                        if (out.length >= 24) break
                    }
                }
                else if (src._nativeProvider === "moewalls") {
                    var cards = src._buf.split("<article ")
                    for (var ci = 0; ci < cards.length; ci++) {
                        var card = cards[ci]
                        if (card.indexOf("g1-frame") < 0) continue
                        var msrc = card.match(new RegExp('src="(https://moewalls\\.com/wp-content/uploads/[0-9]{4}/[0-9]{2}/[^"]*-thumb-[0-9]+x[0-9]+\\.(?:jpe?g|png))"'))
                        if (!msrc) continue
                        var srcUrl = msrc[1]
                        var mhref = card.match(new RegExp('class="g1-frame" href="(https://moewalls\\.com/[^"]*)"'))
                        var mtitle = card.match(new RegExp('<a title="([^"]*)" class="g1-frame"'))
                        var mwres = card.match(/resolutions-([0-9]+x[0-9]+)/)
                        var rel = srcUrl.split("/uploads/")[1]
                        var yy = rel.split("/")[0]
                        var rp = rel.split("/")
                        var mb2 = rp[rp.length - 1].split("-thumb-")[0]
                        var webm = src._mwBase + "/wp-content/uploads/preview/" + yy + "/" + mb2 + "-preview.webm"
                        var nm = mtitle ? mtitle[1].replace(/ Live Wallpaper$/, "") : mb2.replace(/-/g, " ")
                        out.push({ id: mb2,
                                   thumb: "https://wsrv.nl/?url=" + encodeURIComponent(srcUrl),
                                   video: webm, dl: webm,
                                   resolution: mwres ? mwres[1] : "",
                                   name: nm, moewalls_url: (mhref ? mhref[1] : "") })
                    }
                }
            } catch (e) { src.error = "search failed"; src.results = []; return }
            src.error = out.length === 0 ? "no results" : ""
            src.results = out
        }
    }

    property var _nativeDlProc: Process {
        stdout: SplitParser { splitMarker: ""; onRead: function(data) { src._nativeDlBuf += data } }
        onExited: function(code) {
            var p = src._nativeDlBuf.trim()
            src.downloadingId = ""
            if (code === 0 && p.length > 0) src.applied(p)
            else src.failed("download failed")
        }
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
        _nativeDlBuf = ""
        downloadingId = "" + item.id
        if (downloadVerb === "extras-download") {
            var url = "" + (item.dl || item.video || "")
            var ext = url.split(".").pop()
            if (!ext || ext.length > 5) ext = "mp4"
            var dir = Quickshell.env("HOME") + "/Pictures/livewalls"
            var xout = dir + "/ryoku-" + item.id + "." + ext
            _nativeDlProc.command = ["bash", "-lc",
                "mkdir -p " + JSON.stringify(dir) + " && curl -fsSL -A " + JSON.stringify(_ua)
                + " " + JSON.stringify(url) + " -o " + JSON.stringify(xout)
                + " && printf '%s' " + JSON.stringify(xout)]
            _nativeDlProc.running = true
            return
        }
        if (downloadVerb === "motionbgs-download") {
            var mUrl = "" + (item.dl || item.video || "")
            var mDir = Quickshell.env("HOME") + "/Pictures/livewalls"
            var mOut = mDir + "/motionbgs-" + item.id + ".mp4"
            _nativeDlProc.command = ["bash", "-lc",
                "mkdir -p " + JSON.stringify(mDir) + " && curl -fsSL -A " + JSON.stringify(_ua)
                + " -e " + JSON.stringify(_mbBase + "/") + " " + JSON.stringify(mUrl)
                + " -o " + JSON.stringify(mOut) + " && printf '%s' " + JSON.stringify(mOut)]
            _nativeDlProc.running = true
            return
        }
        if (downloadVerb === "moewalls-download") {
            var moeUrl = "" + (item.dl || item.video || "")
            var post = "" + (item.moewalls_url || "")
            var moeDir = Quickshell.env("HOME") + "/Pictures/livewalls"
            var mp4 = moeDir + "/moewalls-" + item.id + ".mp4"
            var wext = moeUrl.split(".").pop(); if (!wext || wext.length > 5) wext = "webm"
            var wf = moeDir + "/moewalls-" + item.id + "." + wext
            var sh = "mkdir -p " + JSON.stringify(moeDir) + "; "
                + "ua=" + JSON.stringify(_ua) + "; post=" + JSON.stringify(post) + "; "
                + "url=" + JSON.stringify(moeUrl) + "; mp4=" + JSON.stringify(mp4) + "; wf=" + JSON.stringify(wf) + "; "
                + "tok=$(curl -fsSL -A \"$ua\" \"$post\" 2>/dev/null | grep -oE '<a[^>]*id=\"moe-download\"[^>]*>' | grep -oE 'data-url=\"[^\"]*\"' | sed -E 's/data-url=\"([^\"]*)\"/\\1/' | head -1); "
                + "if [ -n \"$tok\" ] && curl -fsSL -A \"$ua\" -e \"$post\" \"https://go.moewalls.com/download.php?video=$tok\" -o \"$mp4\" 2>/dev/null; then printf '%s' \"$mp4\"; exit 0; fi; "
                + "if curl -fsSL -A \"$ua\" " + JSON.stringify(moeUrl) + " -o \"$wf\" 2>/dev/null; then printf '%s' \"$wf\"; exit 0; fi; exit 1"
            _nativeDlProc.command = ["bash", "-lc", sh]
            _nativeDlProc.running = true
            return
        }
        var args = [downloadVerb, ("" + item.id), ("" + (item.dl || item.video || ""))]
        if (needsPost && item.moewalls_url) args.push("" + item.moewalls_url)
        _dlProc.command = ["ryowalls"].concat(args)
        _dlProc.running = true
    }
}
