//! Contract: the `wallpaper` topic frame ryoku's QML parses verbatim. A drop or
//! rename here silently breaks the shell, so the exact key set is pinned.

use std::collections::BTreeMap;

use ryogami_proto::{WallFrame, WallFrameEntry};

#[test]
fn wall_frame_serializes_exact_contract_keys() {
    let entry = WallFrameEntry {
        path: "/pics/a.png".into(),
        revision: 7,
        fit: "Cover".into(),
        live: false,
        transition: None,
        depth: "/pics/Depth/a.png".into(),
        depth_rev: 42,
    };
    let mut outputs = BTreeMap::new();
    outputs.insert("DP-1".to_string(), entry.clone());
    let frame = WallFrame { default: entry, outputs };

    let v = serde_json::to_value(&frame).unwrap();

    // Frame shape: exactly {default, outputs}.
    let mut frame_keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    frame_keys.sort_unstable();
    assert_eq!(frame_keys, ["default", "outputs"]);

    // Entry shape: exactly the seven contract keys, incl. camelCase `depthRev`.
    let mut entry_keys: Vec<&str> = v["default"].as_object().unwrap().keys().map(String::as_str).collect();
    entry_keys.sort_unstable();
    assert_eq!(
        entry_keys,
        ["depth", "depthRev", "fit", "live", "path", "revision", "transition"]
    );

    assert_eq!(v["default"]["path"], "/pics/a.png");
    assert_eq!(v["default"]["revision"], 7);
    assert_eq!(v["default"]["fit"], "Cover");
    assert_eq!(v["default"]["live"], false);
    assert!(v["default"]["transition"].is_null());
    assert_eq!(v["default"]["depth"], "/pics/Depth/a.png");
    assert_eq!(v["default"]["depthRev"], 42);
    // The override carries the same contract.
    assert_eq!(v["outputs"]["DP-1"]["depthRev"], 42);
}

#[test]
fn wall_frame_roundtrips_including_transition_object() {
    let entry = WallFrameEntry {
        path: "/x.png".into(),
        revision: 1,
        fit: "Contain".into(),
        live: true,
        transition: Some(serde_json::json!({ "name": "fade", "duration_ms": 300 })),
        depth: String::new(),
        depth_rev: 0,
    };
    let frame = WallFrame { default: entry, outputs: BTreeMap::new() };

    let s = serde_json::to_string(&frame).unwrap();
    let back: WallFrame = serde_json::from_str(&s).unwrap();
    assert_eq!(frame, back);

    // `depthRev` is the wire key; the snake-case field must not leak.
    assert!(s.contains("\"depthRev\""));
    assert!(!s.contains("depth_rev"));
}
