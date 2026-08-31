//! Ryoku line pub/sub: named state topics plus the `wallpaper` surface.
//!
//! This mirrors ryoku-shell's Go `stateTopic`/`wallSurface` (ipc/statestream.go,
//! ipc/wallsurface.go): a subscriber gets the last frame at once, then a fresh
//! frame on every change; byte-identical frames are suppressed. The published
//! `wallpaper` frame is `{default, outputs}` with the entry keys ryoku's QML
//! parses verbatim (see `ryogami_proto::WallFrameEntry`).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use ryogami_proto::{WallFrame, WallFrameEntry};
use tokio::sync::{Mutex, broadcast};

/// A coalescing state topic. Subscribers receive the retained frame immediately,
/// then every subsequent publish. A publish equal to the last frame is dropped so
/// an unchanged value never wakes a binding.
pub struct StateTopic {
    last: Mutex<Option<String>>,
    tx: broadcast::Sender<String>,
}

impl StateTopic {
    fn new() -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(64);
        Arc::new(Self { last: Mutex::new(None), tx })
    }

    pub async fn publish(&self, frame: String) {
        let mut last = self.last.lock().await;
        if last.as_deref() == Some(frame.as_str()) {
            return;
        }
        *last = Some(frame.clone());
        // The lock is held across the send so subscribe() (which takes the same
        // lock before creating its receiver) can never straddle this update.
        let _ = self.tx.send(frame);
    }

    /// The retained frame (if any) plus a receiver for future frames.
    pub async fn subscribe(&self) -> (Option<String>, broadcast::Receiver<String>) {
        let last = self.last.lock().await;
        let rx = self.tx.subscribe();
        (last.clone(), rx)
    }
}

/// Named topic registry. Only `wallpaper` is exposed today.
#[derive(Clone, Default)]
pub struct Topics {
    map: HashMap<String, Arc<StateTopic>>,
}

impl Topics {
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Arc<StateTopic>> {
        self.map.get(name).cloned()
    }
}

/// Render fidelity tier. Drives the renderer's buffer policy in Task 3; stored on
/// the running daemon by `wallpaper resource <low|medium|high>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ResourceTier {
    Low,
    #[default]
    Medium,
    High,
}

impl ResourceTier {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }
}

#[derive(Clone, Default)]
struct WallEntry {
    revision: i64,
    path: String,
    fit: String,
    live: bool,
    transition: Option<serde_json::Value>,
    depth_path: String,
    depth_rev: i64,
}

impl WallEntry {
    fn to_frame(&self) -> WallFrameEntry {
        WallFrameEntry {
            path: self.path.clone(),
            revision: self.revision,
            fit: self.fit.clone(),
            live: self.live,
            transition: self.transition.clone(),
            depth: self.depth_path.clone(),
            depth_rev: self.depth_rev,
        }
    }
}

struct WallState {
    seq: i64,
    def: WallEntry,
    outputs: BTreeMap<String, WallEntry>,
}

/// The in-shell wallpaper surface: a default entry plus per-output overrides,
/// published as one `{default, outputs}` frame on the `wallpaper` topic. A
/// broadcast set writes the default and clears the overrides; a per-output set
/// writes one override. Unlike ryogami's backdrop, Ryogami renders from the source
/// path itself, so the frame carries the source (the bumped `revision` busts any
/// downstream cache); no revision-stamped copy is made.
pub struct WallSurface {
    topic: Arc<StateTopic>,
    state: Mutex<WallState>,
}

impl WallSurface {
    /// Build the surface and a registry exposing its `wallpaper` topic.
    #[must_use]
    pub fn new() -> (Arc<WallSurface>, Topics) {
        let topic = StateTopic::new();
        let surface = Arc::new(WallSurface {
            topic: topic.clone(),
            state: Mutex::new(WallState {
                seq: 0,
                def: WallEntry::default(),
                outputs: BTreeMap::new(),
            }),
        });
        let mut map = HashMap::new();
        map.insert("wallpaper".to_string(), topic);
        (surface, Topics { map })
    }

    async fn publish_locked(&self, st: &WallState) {
        let frame = WallFrame {
            default: st.def.to_frame(),
            outputs: st.outputs.iter().map(|(k, v)| (k.clone(), v.to_frame())).collect(),
        };
        self.topic.publish(serde_json::to_string(&frame).unwrap_or_default()).await;
    }

    /// Publish the current (initially empty) frame so a subscriber that connects
    /// before the first `wallpaper set` still sees a defined frame.
    pub async fn publish_current(&self) {
        let st = self.state.lock().await;
        self.publish_locked(&st).await;
    }

    /// Broadcast set: replace the default and clear every override.
    pub async fn show(&self, pic: &str, fit: &str, transition: Option<serde_json::Value>) {
        let mut st = self.state.lock().await;
        st.seq += 1;
        let rev = st.seq;
        st.def = fresh_entry(rev, pic, fit, transition);
        st.outputs.clear();
        self.publish_locked(&st).await;
    }

    /// Per-output set: write one override, leaving the default and others intact.
    pub async fn show_output(&self, name: &str, pic: &str, fit: &str, transition: Option<serde_json::Value>) {
        if name.is_empty() {
            return;
        }
        let mut st = self.state.lock().await;
        st.seq += 1;
        let rev = st.seq;
        st.outputs.insert(name.to_string(), fresh_entry(rev, pic, fit, transition));
        self.publish_locked(&st).await;
    }

    /// A snapshot of the current frame, for status queries.
    pub async fn snapshot(&self) -> WallFrame {
        let st = self.state.lock().await;
        WallFrame {
            default: st.def.to_frame(),
            outputs: st.outputs.iter().map(|(k, v)| (k.clone(), v.to_frame())).collect(),
        }
    }

    /// Publish a slot's cutout unless a switch mid-generation already moved it to
    /// another wallpaper. `rev` is the cutout's mtime, so a regenerated file at
    /// the same path still busts the image cache.
    pub async fn set_depth(&self, slot: &str, source: &str, out: &str, rev: i64) {
        let mut st = self.state.lock().await;
        {
            let entry = if slot.is_empty() {
                &mut st.def
            } else {
                match st.outputs.get_mut(slot) {
                    Some(e) => e,
                    None => return,
                }
            };
            if entry.path != source {
                return;
            }
            entry.depth_path = out.to_string();
            entry.depth_rev = rev;
        }
        self.publish_locked(&st).await;
    }

    /// Drop every slot's cutout (depth disabled or the engine gone), publishing
    /// once if anything changed.
    pub async fn clear_depth(&self) {
        let mut st = self.state.lock().await;
        let mut changed = false;
        if !st.def.depth_path.is_empty() {
            st.def.depth_path.clear();
            st.def.depth_rev = 0;
            changed = true;
        }
        for e in st.outputs.values_mut() {
            if !e.depth_path.is_empty() {
                e.depth_path.clear();
                e.depth_rev = 0;
                changed = true;
            }
        }
        if changed {
            self.publish_locked(&st).await;
        }
    }
}

fn fresh_entry(rev: i64, pic: &str, fit: &str, transition: Option<serde_json::Value>) -> WallEntry {
    WallEntry {
        revision: rev,
        path: pic.to_string(),
        fit: fit.to_string(),
        // A fresh frame is the renderer's to paint until a live player claims it.
        live: false,
        transition,
        // A fresh wallpaper needs a fresh cutout; the depth worker regenerates it.
        depth_path: String::new(),
        depth_rev: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn show_publishes_bumped_revision_on_default() {
        let (surface, topics) = WallSurface::new();
        surface.publish_current().await;
        let topic = topics.get("wallpaper").expect("wallpaper topic");
        let (initial, mut rx) = topic.subscribe().await;

        let empty: WallFrame = serde_json::from_str(&initial.unwrap()).unwrap();
        assert_eq!(empty.default.revision, 0);
        assert_eq!(empty.default.path, "");

        surface.show("/img/a.png", "Cover", None).await;
        let frame: WallFrame = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(frame.default.path, "/img/a.png");
        assert_eq!(frame.default.fit, "Cover");
        assert!(frame.default.revision > 0);
        assert!(frame.outputs.is_empty());
    }

    #[tokio::test]
    async fn show_output_writes_one_override_and_keeps_default() {
        let (surface, topics) = WallSurface::new();
        let topic = topics.get("wallpaper").unwrap();
        surface.show("/img/def.png", "Cover", None).await;
        surface.show_output("DP-1", "/img/dp1.png", "Contain", None).await;
        let (last, _rx) = topic.subscribe().await;
        let frame: WallFrame = serde_json::from_str(&last.unwrap()).unwrap();
        assert_eq!(frame.default.path, "/img/def.png");
        assert_eq!(frame.outputs["DP-1"].path, "/img/dp1.png");
        assert_eq!(frame.outputs["DP-1"].fit, "Contain");
        assert!(frame.outputs["DP-1"].revision > frame.default.revision);
    }

    #[tokio::test]
    async fn identical_frame_is_suppressed() {
        let topic = StateTopic::new();
        topic.publish("same".to_string()).await;
        let (_last, mut rx) = topic.subscribe().await;
        topic.publish("same".to_string()).await;
        topic.publish("different".to_string()).await;
        assert_eq!(rx.recv().await.unwrap(), "different");
    }
}
