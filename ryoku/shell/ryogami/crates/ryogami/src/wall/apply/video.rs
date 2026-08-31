use std::collections::HashMap;
use std::path::PathBuf;

use tracing::info;

use crate::config::Config;

use super::*;

pub async fn apply_video(
    path: &str,
    outputs: &[String],
    neighbors: &[String],
    all_screens: &[String],
    outputs_audio: &HashMap<String, bool>,
    outputs_volume: &HashMap<String, u32>,
    config: &Config,
) -> anyhow::Result<()> {
    let _ = all_screens;
    apply_video_inner(path, outputs, neighbors, outputs_audio, outputs_volume, config, false).await
}

pub(super) async fn apply_video_inner(
    path: &str,
    outputs: &[String],
    _neighbors: &[String], // Task 4: transition thumbnail neighbours
    outputs_audio: &HashMap<String, bool>,
    outputs_volume: &HashMap<String, u32>,
    config: &Config,
    restoring: bool,
) -> anyhow::Result<()> {
    let apply_total = std::time::Instant::now();
    let lock_wait = std::time::Instant::now();
    let _apply_guard = apply_lock().lock().await;
    let lock_wait_ms = lock_wait.elapsed().as_millis() as u64;
    let is_kde = is_kde();
    let mute = config.is_muted();
    let prev_was_we = linux_we_running().await;

    let dedup_target_outs: Vec<String> = if outputs.is_empty() {
        vec!["*".to_string()]
    } else {
        outputs.to_vec()
    };
    let dedup_mute = compute_audio_dedup(
        &config.cache_dir(),
        &dedup_target_outs,
        outputs_audio,
        "video",
        path,
        "",
        mute,
    )
    .await;

    if !prev_was_we {
        kill_legacy_video_procs().await;
    }

    let thumb_path: Option<PathBuf> = ensure_video_thumb_blocking(path, &config.cache_dir()).await;
    let thumb_str = thumb_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let matugen_handle = thumb_path.as_ref().map(|thumb| {
        let thumb = thumb.clone();
        let cfg = config.clone();
        tokio::spawn(async move {
            let wd_cache = cfg.cache_dir().join("wallpaper");
            let _ = tokio::fs::create_dir_all(&wd_cache).await;
            let _ = tokio::fs::copy(&thumb, wd_cache.join("current.jpg")).await;
            run_matugen(thumb.to_str().unwrap_or(""), &cfg).await;
            run_reloads(&cfg).await;
        })
    });

    if config.wants_external_render() {
        stop_all().await;
        let _ = tokio::fs::remove_file(config.video_dir().join("lockscreen-video.mp4")).await;
    } else if is_kde {
        let kde_target_outs: Vec<String> = if outputs.is_empty() {
            vec!["*".to_string()]
        } else {
            outputs.to_vec()
        };
        let _ = rebuild_scene_we(
            config,
            &kde_target_outs,
            &std::collections::BTreeMap::new(),
            config.volume(),
        )
        .await;
        stop_all().await;
        apply_kde_video(path, outputs, &dedup_mute, outputs_volume, config).await?;
    } else {
        let global_volume = config.volume();
        let target_outs: Vec<String> = dedup_target_outs.clone();
        let mute_for = |out: &str| -> bool { dedup_mute.get(out).copied().unwrap_or(mute) };
        let volume_for = |out: &str| -> u32 { outputs_volume.get(out).copied().unwrap_or(global_volume) };

        rebuild_scene_we(
            config,
            &target_outs,
            &std::collections::BTreeMap::new(),
            global_volume,
        )
        .await
        .ok();

        // Start a livewall on each target output. Audio dedup (one audible output
        // per group) is enforced below via `enforce_audio_dedup`. Transitions land
        // in Task 4.
        let entries: Vec<(String, bool, u32)> = target_outs
            .iter()
            .map(|out| (out.clone(), mute_for(out), volume_for(out)))
            .collect();
        show_video(&entries, path, config.display.fill_mode).await;
        info!(outputs = target_outs.len(), "apply_video: in-process livewall");
    }

    if prev_was_we {
        kill_legacy_video_procs().await;
    }

    save_state(&config.cache_dir(), "video", path, "").await;
    let prev_outputs_state = read_outputs_state(&config.cache_dir()).await;
    save_outputs_state(&config.cache_dir(), outputs, "video", path, "", &dedup_mute).await;
    if !outputs.is_empty() && !outputs.iter().any(|o| o == "*") {
        mute_wildcard_if_present(config).await;
    }
    preserve_group_audio(config, &prev_outputs_state).await;
    enforce_audio_dedup(config).await;

    let path = path.to_string();
    let config = config.clone();
    tokio::spawn(async move {
        if let Some(handle) = matugen_handle {
            let _ = handle.await;
        }
        if config.wants_external_render() {
            run_external_apply(&config, "video", &path, &thumb_str).await;
        }
        run_post_processing(&config, "video", &basename(&path), &path, &thumb_str, restoring).await;
        info!("post-apply tasks done for video: {path}");
    });

    info!(
        total_ms = apply_total.elapsed().as_millis() as u64,
        lock_wait_ms,
        "applied video wallpaper"
    );
    Ok(())
}
