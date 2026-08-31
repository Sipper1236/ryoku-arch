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
