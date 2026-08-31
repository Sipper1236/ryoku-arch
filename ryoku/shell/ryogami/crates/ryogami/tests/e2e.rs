//! Wiring proof: the real `ryogami` binary serves the ryoku line pub/sub. A
//! subscriber to the `wallpaper` topic must receive a fresh frame after a
//! `wallpaper set` command, carrying the applied path and a bumped revision.
//!
//! Run headless (`RYOGAMI_HEADLESS=1`) so no renderer is spawned: the assertion
//! is the topic publish, not painted pixels.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Daemon {
    child: Child,
    _dir: tempfile::TempDir,
    sock: PathBuf,
    root: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_daemon() -> Daemon {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let runtime = root.join("run");
    std::fs::create_dir_all(&runtime).unwrap();

    let child = Command::new(env!("CARGO_BIN_EXE_ryogami"))
        .env("RYOGAMI_HEADLESS", "1")
        .env("HOME", &root)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ryogami");

    let sock = runtime.join("ryogami.sock");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !sock.exists() {
        assert!(Instant::now() < deadline, "daemon socket never appeared at {}", sock.display());
        std::thread::sleep(Duration::from_millis(50));
    }
    Daemon { child, _dir: dir, sock, root }
}

fn connect(sock: &Path) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match UnixStream::connect(sock) {
            Ok(s) => return s,
            Err(e) => {
                assert!(Instant::now() < deadline, "connect failed: {e}");
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

fn read_frame(reader: &mut BufReader<UnixStream>) -> serde_json::Value {
    let mut line = String::new();
    let n = reader.read_line(&mut line).expect("read frame");
    assert!(n > 0, "eof while waiting for a frame");
    serde_json::from_str(line.trim()).expect("frame is not JSON")
}

// One-shot helper: send a line, read one reply. The daemon holds the connection
// open for more requests, but closing early is a supported client style.
fn send_command(sock: &Path, line: &str) -> String {
    let cmd = connect(sock);
    cmd.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut w = cmd.try_clone().unwrap();
    let mut r = BufReader::new(cmd);
    w.write_all(line.as_bytes()).unwrap();
    w.write_all(b"\n").unwrap();
    w.flush().unwrap();
    let mut reply = String::new();
    r.read_line(&mut reply).unwrap();
    reply.trim().to_string()
}

#[test]
fn wallpaper_topic_publishes_frame_on_set() {
    let d = start_daemon();

    // 1) Subscribe: the retained (empty) frame arrives first, proving the topic.
    let sub = connect(&d.sock);
    sub.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut sub_w = sub.try_clone().unwrap();
    let mut sub_r = BufReader::new(sub);
    sub_w.write_all(b"subscribe wallpaper\n").unwrap();
    sub_w.flush().unwrap();

    let initial = read_frame(&mut sub_r);
    let init_rev = initial["default"]["revision"].as_i64().expect("initial revision");

    // 2) Apply a static image on a separate connection.
    let img = d.root.join("wall.png");
    std::fs::write(&img, b"placeholder").unwrap();
    let img_str = img.display().to_string();

    let cmd = connect(&d.sock);
    cmd.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut cmd_w = cmd.try_clone().unwrap();
    let mut cmd_r = BufReader::new(cmd);
    cmd_w.write_all(format!("wallpaper set {img_str}\n").as_bytes()).unwrap();
    cmd_w.flush().unwrap();
    let mut reply = String::new();
    cmd_r.read_line(&mut reply).unwrap();
    assert_eq!(reply.trim(), "ok", "set reply");

    // 3) The subscription receives the new frame: our path, a bumped revision,
    //    and the exact contract keys ryoku's QML parses.
    let frame = read_frame(&mut sub_r);
    let def = &frame["default"];
    assert_eq!(def["path"].as_str().unwrap(), img_str, "frame path");
    assert!(def["revision"].as_i64().unwrap() > init_rev, "revision bumped");
    for key in ["path", "revision", "fit", "live", "transition", "depth", "depthRev"] {
        assert!(def.get(key).is_some(), "entry missing contract key: {key}");
    }
    assert!(frame.get("outputs").is_some(), "frame missing outputs map");
}

#[test]
fn subscribe_to_unknown_topic_errors() {
    let d = start_daemon();
    let s = connect(&d.sock);
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut w = s.try_clone().unwrap();
    let mut r = BufReader::new(s);
    w.write_all(b"subscribe nope\n").unwrap();
    w.flush().unwrap();
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    assert!(line.trim().starts_with("err"), "expected err, got {line:?}");
}

#[test]
fn cli_wallpaper_set_publishes_frame() {
    // The real `ryogami` binary, given a subcommand, acts as a client: it connects
    // to ryogami.sock, sends the line command, and a topic subscriber sees the frame.
    let d = start_daemon();
    let runtime = d.sock.parent().unwrap().to_path_buf();

    let sub = connect(&d.sock);
    sub.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut sub_w = sub.try_clone().unwrap();
    let mut sub_r = BufReader::new(sub);
    sub_w.write_all(b"subscribe wallpaper\n").unwrap();
    sub_w.flush().unwrap();
    let init_rev = read_frame(&mut sub_r)["default"]["revision"].as_i64().unwrap();

    let img = d.root.join("cli.png");
    std::fs::write(&img, b"placeholder").unwrap();
    let img_str = img.display().to_string();

    let out = Command::new(env!("CARGO_BIN_EXE_ryogami"))
        .arg("wallpaper")
        .arg("set")
        .arg(&img_str)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("HOME", &d.root)
        .output()
        .expect("run ryogami client");
    assert!(
        out.status.success(),
        "client exit={:?} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok", "client prints daemon reply");

    let frame = read_frame(&mut sub_r);
    assert_eq!(frame["default"]["path"].as_str().unwrap(), img_str, "frame carries the CLI's path");
    assert!(frame["default"]["revision"].as_i64().unwrap() > init_rev, "revision bumped");
}

#[test]
fn wallpaper_repaint_reemits_a_frame() {
    // repaint re-derives theme/borders and re-fits in place after a filter change:
    // the subscriber must see a fresh frame (same path, bumped revision) so the
    // re-rendered image is reloaded — a byte-identical frame would be suppressed.
    let d = start_daemon();
    let sub = connect(&d.sock);
    sub.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut sub_w = sub.try_clone().unwrap();
    let mut sub_r = BufReader::new(sub);
    sub_w.write_all(b"subscribe wallpaper\n").unwrap();
    sub_w.flush().unwrap();
    read_frame(&mut sub_r); // retained (empty) frame

    let img = d.root.join("wall.png");
    std::fs::write(&img, b"placeholder").unwrap();
    let img_str = img.display().to_string();

    assert_eq!(send_command(&d.sock, &format!("wallpaper set {img_str}")), "ok", "set reply");
    let set_rev = read_frame(&mut sub_r)["default"]["revision"].as_i64().unwrap();

    assert_eq!(send_command(&d.sock, "wallpaper repaint"), "ok", "repaint reply");

    let frame = read_frame(&mut sub_r);
    assert_eq!(frame["default"]["path"].as_str().unwrap(), img_str, "repaint keeps the same path");
    assert!(frame["default"]["revision"].as_i64().unwrap() > set_rev, "repaint re-emits with a bumped revision");
}

#[test]
fn wallpaper_next_advances_to_a_different_wallpaper() {
    // `next` advances to the following file in the wallpaper dir: the published
    // frame must carry a different path than the current one.
    let d = start_daemon();
    let walls = d.root.join("Pictures").join("Wallpapers");
    std::fs::create_dir_all(&walls).unwrap();
    let a = walls.join("a.png");
    let b = walls.join("b.png");
    std::fs::write(&a, b"a").unwrap();
    std::fs::write(&b, b"b").unwrap();
    let a_str = a.display().to_string();
    let b_str = b.display().to_string();

    let sub = connect(&d.sock);
    sub.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut sub_w = sub.try_clone().unwrap();
    let mut sub_r = BufReader::new(sub);
    sub_w.write_all(b"subscribe wallpaper\n").unwrap();
    sub_w.flush().unwrap();
    read_frame(&mut sub_r); // retained (empty) frame

    assert_eq!(send_command(&d.sock, &format!("wallpaper set {a_str}")), "ok", "set reply");
    let set_frame = read_frame(&mut sub_r);
    assert_eq!(set_frame["default"]["path"].as_str().unwrap(), a_str, "current is a.png");

    assert_eq!(send_command(&d.sock, "wallpaper next"), "ok", "next reply");

    let next_frame = read_frame(&mut sub_r);
    let picked = next_frame["default"]["path"].as_str().unwrap();
    assert_eq!(picked, b_str, "next advanced to the following wallpaper");
    assert_ne!(picked, a_str, "next picked a different wallpaper than current");
}

#[test]
fn json_requests_and_events_share_one_connection() {
    // The wall-ui contract: a persistent connection answers JSON requests with
    // full Response payloads, and after a JSON `subscribe` it also pushes
    // broadcast events (ryogami.wall.*) interleaved between replies.
    let d = start_daemon();
    let walls = d.root.join("Pictures").join("Wallpapers");
    std::fs::create_dir_all(&walls).unwrap();
    let img = walls.join("a.png");
    std::fs::write(&img, b"a").unwrap();
    let img_str = img.display().to_string();

    let conn = connect(&d.sock);
    conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut w = conn.try_clone().unwrap();
    let mut r = BufReader::new(conn);
    let mut read_json = |r: &mut BufReader<UnixStream>| -> serde_json::Value {
        let mut line = String::new();
        assert!(r.read_line(&mut line).expect("read reply") > 0, "eof");
        serde_json::from_str(line.trim()).expect("reply is not JSON")
    };

    // 1) A JSON request gets the full Response, result payload included.
    w.write_all(b"{\"method\":\"status\",\"id\":7}\n").unwrap();
    w.flush().unwrap();
    let status = read_json(&mut r);
    assert_eq!(status["id"].as_i64(), Some(7), "response echoes the request id");
    assert!(status.get("result").is_some(), "status carries a result payload");

    // 2) Same connection, second request: the daemon does not hang up.
    w.write_all(b"{\"method\":\"wall.list\",\"id\":8}\n").unwrap();
    w.flush().unwrap();
    let list = read_json(&mut r);
    assert_eq!(list["id"].as_i64(), Some(8));
    assert!(list["result"].get("wallpapers").is_some(), "wall.list returns the wallpapers array");

    // 3) Subscribe, then trigger an apply elsewhere: the event is pushed here.
    w.write_all(b"{\"method\":\"subscribe\",\"params\":{\"prefixes\":[\"ryogami.\"]},\"id\":9}\n").unwrap();
    w.flush().unwrap();
    let sub = read_json(&mut r);
    assert_eq!(sub["id"].as_i64(), Some(9));
    assert!(sub["result"].get("subscribed").is_some(), "subscribe acks its prefixes");

    // wall.toggle always broadcasts ryogami.wall.toggle (headless skips the
    // actual quickshell spawn), so the push path is provable without a renderer.
    let toggled = send_command(&d.sock, "{\"method\":\"wall.toggle\",\"id\":10}");
    assert!(toggled.contains("\"toggled\""), "toggle replied: {toggled}");
    let ev = read_json(&mut r);
    assert_eq!(ev["event"].as_str(), Some("ryogami.wall.toggle"), "pushed event");
    assert!(ev["data"].get("visible").is_some(), "toggle event carries visible");
}
