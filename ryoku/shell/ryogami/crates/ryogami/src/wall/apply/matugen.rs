use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::collections::BTreeMap;

use tokio::process::Command;
use tracing::{info, warn};

use crate::config::{self, Config};
use crate::util::{CommandExt, CommandRunner, CommandSpec};

use super::external::{run_sh, shell_quote};

pub async fn theme_full_palette(
    runner: &dyn CommandRunner,
    config: &Config,
    scheme: &str,
    mode: &str,
    color_index: u32,
) -> anyhow::Result<serde_json::Value> {
    let current_jpg = config.cache_dir().join("wallpaper/current.jpg");
    if !current_jpg.exists() {
        anyhow::bail!("no current wallpaper image for palette");
    }
    let spec = CommandSpec::new("matugen").args([
        "--dry-run".to_string(),
        "-j".to_string(),
        "hex".to_string(),
        "--prefer".to_string(),
        "saturation".to_string(),
        "image".to_string(),
        "-t".to_string(),
        scheme.to_string(),
        "-m".to_string(),
        mode.to_string(),
        "--source-color-index".to_string(),
        color_index.to_string(),
        current_jpg.display().to_string(),
    ]);
    let out = runner.run(spec).await?;
    if !out.status.success() {
        let err_msg = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("matugen failed: {} stderr={}", out.status, err_msg.trim());
    }
    let raw: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    Ok(build_full_palette(&raw, scheme, mode, color_index))
}

fn build_full_palette(
    raw: &serde_json::Value,
    scheme: &str,
    mode: &str,
    color_index: u32,
) -> serde_json::Value {
    let mut colors_out = serde_json::Map::new();
    if let Some(obj) = raw.get("colors").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            let hex = v
                .get("default")
                .and_then(|d| d.get("color"))
                .and_then(|c| c.as_str())
                .map(std::string::ToString::to_string);
            if let Some(h) = hex {
                colors_out.insert(k.clone(), serde_json::Value::String(h));
            }
        }
    }

    let mut palettes_out = serde_json::Map::new();
    if let Some(obj) = raw.get("palettes").and_then(|v| v.as_object()) {
        for (family, tones) in obj {
            let mut tone_map = serde_json::Map::new();
            if let Some(t_obj) = tones.as_object() {
                for (tone_key, tone_val) in t_obj {
                    let hex = tone_val
                        .get("color")
                        .and_then(|c| c.as_str())
                        .map(std::string::ToString::to_string);
                    if let Some(h) = hex {
                        tone_map.insert(tone_key.clone(), serde_json::Value::String(h));
                    }
                }
            }
            palettes_out.insert(family.clone(), serde_json::Value::Object(tone_map));
        }
    }

    serde_json::json!({
        "scheme": scheme,
        "mode": mode,
        "color_index": color_index,
        "is_dark_mode": raw.get("is_dark_mode").cloned().unwrap_or_else(|| serde_json::json!(mode == "dark")),
        "source_color": raw.get("colors").and_then(|c| c.get("source_color"))
                          .and_then(|s| s.get("default")).and_then(|d| d.get("color"))
                          .cloned().unwrap_or_else(|| serde_json::json!("")),
        "colors": serde_json::Value::Object(colors_out),
        "palettes": serde_json::Value::Object(palettes_out),
    })
}

pub async fn theme_preview(
    config: &Config,
    scheme: &str,
    mode: &str,
    color_index: u32,
) -> anyhow::Result<serde_json::Value> {
    let current_jpg = config.cache_dir().join("wallpaper/current.jpg");
    if !current_jpg.exists() {
        anyhow::bail!("no current wallpaper image for preview");
    }
    let out = Command::new("matugen")
        .arg("--dry-run")
        .arg("-j")
        .arg("hex")
        .arg("image")
        .arg("-t")
        .arg(scheme)
        .arg("-m")
        .arg(mode)
        .arg("--source-color-index")
        .arg(color_index.to_string())
        .arg(current_jpg.display().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    if !out.status.success() {
        let err_msg = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!(
            "matugen preview failed: status {} stderr={}",
            out.status,
            err_msg.trim()
        );
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let pick = |key: &str| -> String {
        json.get("colors")
            .and_then(|c| c.get(key))
            .and_then(|e| e.get("default"))
            .and_then(|d| d.get("color"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    Ok(serde_json::json!({
        "scheme": scheme,
        "mode": mode,
        "color_index": color_index,
        "primary": pick("primary"),
        "secondary": pick("secondary"),
        "tertiary": pick("tertiary"),
        "surface": pick("surface"),
        "background": pick("background"),
        "on_surface": pick("on_surface"),
    }))
}


pub(super) async fn generate_matugen_config(config: &Config) -> PathBuf {
    let config_path = config.matugen_config_path();
    let template_dir = config.template_dir();
    let cache_dir = config.cache_dir();

    let mut lines = vec!["[config]".to_string(), "reload_apps = false".to_string(), String::new()];
    let mut emitted = 0usize;

    for (i, integ) in config.integrations.iter().enumerate() {
        let template = match &integ.template {
            Some(t) if !t.is_empty() => t,
            _ => continue,
        };
        let output = match &integ.output {
            Some(o) if !o.is_empty() => o,
            _ => continue,
        };

        let input_path = if template.contains('/') {
            config::resolve_tilde(template)
        } else {
            template_dir.join(template)
        };

        if !input_path.exists() {
            warn!(
                "matugen integration '{}' template not found at {}, skipping",
                integ.name.as_deref().unwrap_or("(unnamed)"),
                input_path.display()
            );
            continue;
        }

        let output_path = if output.contains('/') {
            config::resolve_tilde(output)
        } else {
            cache_dir.join(output)
        };

        let safe_name = integ
            .name
            .as_deref()
            .unwrap_or(&format!("integration_{i}"))
            .replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "_");

        lines.push(format!("[templates.{safe_name}]"));
        lines.push(format!("input_path = \"{}\"", input_path.display()));
        lines.push(format!("output_path = \"{}\"", output_path.display()));
        lines.push(String::new());
        emitted += 1;
    }

    if emitted == 0 {
        lines.push("[templates]".to_string());
    }

    let _ = tokio::fs::create_dir_all(config_path.parent().unwrap_or_else(|| Path::new("/tmp"))).await;
    let _ = tokio::fs::write(&config_path, lines.join("\n")).await;
    info!("generated matugen config with {emitted} integrations");
    config_path
}

pub(super) async fn run_matugen(image_path: &str, config: &Config) {
    if !config.features.matugen {
        return;
    }

    if Command::new("command")
        .arg("-v")
        .arg("matugen")
        .silent()
        .status()
        .await
        .map(|s| !s.success())
        .unwrap_or(true)
        && Command::new("which")
            .arg("matugen")
            .silent()
            .status()
            .await
            .map(|s| !s.success())
            .unwrap_or(true)
    {
        warn!("matugen not found in PATH, skipping");
        return;
    }

    let config_path = generate_matugen_config(config).await;
    let scheme = config.matugen_scheme();
    let mode = config.matugen_mode();
    let color_index = config.matugen_color_index();
    run_matugen_inner(image_path, config, &config_path, scheme, mode, color_index).await;
}

pub(super) async fn run_matugen_with(
    image_path: &str,
    config: &Config,
    scheme: Option<&str>,
    mode: Option<&str>,
    color_index: Option<u32>,
) {
    if !config.features.matugen {
        return;
    }
    let config_path = generate_matugen_config(config).await;
    let scheme = scheme.unwrap_or_else(|| config.matugen_scheme());
    let mode = mode.unwrap_or_else(|| config.matugen_mode());
    let color_index = color_index.unwrap_or_else(|| config.matugen_color_index());
    run_matugen_inner(image_path, config, &config_path, scheme, mode, color_index).await;
}


// --- ~/.cache/ryoku/colors.json ---------------------------------------------
//
// Ryogami owns the wallpaper-derived palette (the seam: ryoku-shell owns
// named-scheme theming). On every apply the daemon authors the one palette file
// every Quickshell `Scheme` singleton reads: base16 for the legacy readers plus
// the camelCase Material 3 roles. The key set mirrors ryoku's Go
// `matugenColorsJSON` so the file stays byte-shape compatible.

/// matugen's snake_case Material 3 roles -> the camelCase keys the shell reads.
const ROLE_KEYS: &[(&str, &str)] = &[
    ("surface", "surface"),
    ("surface_variant", "surfaceVariant"),
    ("surface_container_lowest", "surfaceContainerLowest"),
    ("surface_container_low", "surfaceContainerLow"),
    ("surface_container", "surfaceContainer"),
    ("surface_container_high", "surfaceContainerHigh"),
    ("surface_container_highest", "surfaceContainerHighest"),
    ("inverse_surface", "inverseSurface"),
    ("inverse_on_surface", "inverseOnSurface"),
    ("surface_tint", "surfaceTint"),
    ("primary", "primary"),
    ("primary_container", "primaryContainer"),
    ("secondary", "secondary"),
    ("secondary_container", "secondaryContainer"),
    ("tertiary", "tertiary"),
    ("tertiary_container", "tertiaryContainer"),
    ("error", "error"),
    ("error_container", "errorContainer"),
    ("outline", "outline"),
    ("outline_variant", "outlineVariant"),
    ("on_surface", "onSurface"),
    ("on_surface_variant", "onSurfaceVariant"),
    ("on_primary", "onPrimary"),
    ("on_primary_container", "onPrimaryContainer"),
    ("on_secondary", "onSecondary"),
    ("on_secondary_container", "onSecondaryContainer"),
    ("on_tertiary", "onTertiary"),
    ("on_tertiary_container", "onTertiaryContainer"),
    ("on_error", "onError"),
    ("on_error_container", "onErrorContainer"),
    ("shadow", "shadow"),
    ("scrim", "scrim"),
];

/// The sixteen base16 slots, mapped from the Material 3 palette exactly as ryoku's
/// Go `matugenBase16` does so both palette sources read alike.
fn base16(pal: &BTreeMap<String, String>) -> serde_json::Map<String, serde_json::Value> {
    let pick = |k: &str, fallback: &str| -> String {
        pal.get(k).filter(|v| !v.is_empty()).cloned().unwrap_or_else(|| fallback.to_string())
    };
    let bg = pick("surface", &pick("background", "#121212"));
    let fg = pick("on_surface", &pick("on_background", "#e6e6e6"));
    let primary = pick("primary", "#a8c7fa");
    let secondary = pick("secondary", "#7cacf8");
    let tertiary = pick("tertiary", "#ffb4a9");
    let errc = pick("error", "#ffb4ab");
    let outline = pick("outline", "#8e918f");
    let c15 = pick("on_primary_container", &fg);
    let slots: [(&str, &str); 19] = [
        ("background", bg.as_str()),
        ("foreground", fg.as_str()),
        ("cursor", fg.as_str()),
        ("color0", bg.as_str()),
        ("color1", errc.as_str()),
        ("color2", primary.as_str()),
        ("color3", tertiary.as_str()),
        ("color4", secondary.as_str()),
        ("color5", primary.as_str()),
        ("color6", tertiary.as_str()),
        ("color7", fg.as_str()),
        ("color8", outline.as_str()),
        ("color9", errc.as_str()),
        ("color10", primary.as_str()),
        ("color11", tertiary.as_str()),
        ("color12", secondary.as_str()),
        ("color13", primary.as_str()),
        ("color14", outline.as_str()),
        ("color15", c15.as_str()),
    ];
    slots
        .iter()
        .map(|(k, v)| ((*k).to_string(), serde_json::Value::String((*v).to_string())))
        .collect()
}

/// base16 slots plus the camelCase Material 3 roles: the colors.json body.
fn colors_json(pal: &BTreeMap<String, String>) -> serde_json::Value {
    let mut out = base16(pal);
    for (snake, camel) in ROLE_KEYS {
        if let Some(v) = pal.get(*snake)
            && !v.is_empty()
        {
            out.insert((*camel).to_string(), serde_json::Value::String(v.clone()));
        }
    }
    serde_json::Value::Object(out)
}

/// Flatten matugen's `-j hex` output (`colors.<role>.default.color`) into a
/// `role -> hex` map.
fn flat_palette(raw: &serde_json::Value) -> BTreeMap<String, String> {
    let mut pal = BTreeMap::new();
    if let Some(obj) = raw.get("colors").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            if let Some(hex) = v.get("default").and_then(|d| d.get("color")).and_then(|c| c.as_str()) {
                pal.insert(k.clone(), hex.to_string());
            }
        }
    }
    pal
}

/// Run matugen once in dry-run to read the palette for the applied image, mirroring
/// the apply's scheme/mode/index/contrast so colors.json matches the rendered set.
async fn extract_palette(
    image_path: &str,
    config: &Config,
    scheme: &str,
    mode: &str,
    color_index: u32,
) -> Option<serde_json::Value> {
    let mut command = Command::new("matugen");
    command
        .arg("--dry-run")
        .arg("-j")
        .arg("hex")
        .arg("image")
        .arg("-t")
        .arg(scheme)
        .arg("-m")
        .arg(mode)
        .arg("--source-color-index")
        .arg(color_index.to_string());
    if let Some(contrast) = config.matugen_contrast() {
        command.arg("--contrast").arg(contrast.to_string());
    }
    let out = command
        .arg(image_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        warn!("matugen palette extract failed: {}", String::from_utf8_lossy(&out.stderr).trim());
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// Author `~/.cache/ryoku/colors.json` from the applied image. Coalesced with the
/// rest of the matugen worker (the apply path themes the final wallpaper of a
/// burst); written via temp + rename so a reader never sees a half-written file.
async fn write_ryoku_colors(image_path: &str, config: &Config, scheme: &str, mode: &str, color_index: u32) {
    let Some(raw) = extract_palette(image_path, config, scheme, mode, color_index).await else {
        return;
    };
    let pal = flat_palette(&raw);
    if pal.is_empty() {
        return;
    }
    let path = config.colors_path();
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let text = serde_json::to_string_pretty(&colors_json(&pal)).unwrap_or_default();
    let tmp = path.with_extension("json.tmp");
    if tokio::fs::write(&tmp, text).await.is_err() {
        return;
    }
    if tokio::fs::rename(&tmp, &path).await.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
        return;
    }
    info!("wrote wallpaper palette to {}", path.display());
}

pub(super) async fn run_matugen_inner(
    image_path: &str,
    config: &Config,
    config_path: &Path,
    scheme: &str,
    mode: &str,
    color_index: u32,
) {
    // Ryogami owns the wallpaper palette: author colors.json before fanning the
    // same palette into the app templates below.
    write_ryoku_colors(image_path, config, scheme, mode, color_index).await;
    let mut command = Command::new("matugen");
    command
        .arg("-c")
        .arg(config_path)
        .arg("image")
        .arg("-t")
        .arg(scheme)
        .arg("-m")
        .arg(mode)
        .arg("--source-color-index")
        .arg(color_index.to_string());
    if let Some(contrast) = config.matugen_contrast() {
        command.arg("--contrast").arg(contrast.to_string());
    }
    let status = command
        .arg(image_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .await;

    match status {
        Ok(s) if s.success() => info!("matugen completed for {image_path}"),
        Ok(s) => warn!("matugen exited with {s} for {image_path}"),
        Err(e) => warn!("failed to run matugen: {e}"),
    }

    match config.default_matugen_config_path() {
        Some(cfg) if !cfg.exists() => {
            warn!("external matugen: config {} does not exist, skipping user templates", cfg.display());
        }
        None => {
            warn!("external matugen: no config path resolved, skipping user templates");
        }
        _ => {}
    }

    if let Some(default_cfg) = config.default_matugen_config_path()
        && default_cfg.exists()
    {
        let default_cmd = "matugen -c %config% image %path% -t %scheme% -m %mode% --source-color-index %index%";
        let cmd_template = match config.external_matugen_command.as_deref() {
            Some(s) if !s.trim().is_empty() => s,
            _ => default_cmd,
        };
        let cmd = cmd_template
            .replace("%config%", &shell_quote(&default_cfg.display().to_string()))
            .replace("%path%", &shell_quote(image_path))
            .replace("%scheme%", &shell_quote(scheme))
            .replace("%mode%", &shell_quote(mode))
            .replace("%index%", &color_index.to_string());
        info!("running external matugen: {cmd}");
        if let Err(e) = run_sh(&cmd).await {
            warn!("failed to run external matugen: {e}");
        }
    }
}

pub(super) async fn run_reloads(config: &Config) {
    for integ in &config.integrations {
        let reload = match &integ.reload {
            Some(r) if !r.is_empty() => r,
            _ => continue,
        };

        let resolved = config::resolve_tilde(reload);
        let cmd = if resolved.to_str().is_some_and(|s| s.contains('/') && !s.contains(' ')) {
            format!("sh {}", shell_quote(&resolved.display().to_string()))
        } else {
            reload.clone()
        };

        info!("running reload: {cmd}");
        let _ = run_sh(&cmd).await;
    }

    if config.general.notify_on_wallpaper_change {
        let _ = run_sh("command -v notify-send >/dev/null && notify-send 'Wallpaper Changed' || true").await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Integration;

    #[test]
    fn colors_json_maps_base16_slots_and_m3_roles() {
        let mut pal = BTreeMap::new();
        pal.insert("surface".to_string(), "#111111".to_string());
        pal.insert("on_surface".to_string(), "#eeeeee".to_string());
        pal.insert("primary".to_string(), "#aabbcc".to_string());
        pal.insert("secondary".to_string(), "#445566".to_string());
        pal.insert("tertiary".to_string(), "#778899".to_string());
        pal.insert("outline".to_string(), "#888888".to_string());
        pal.insert("error".to_string(), "#ff0000".to_string());
        let v = colors_json(&pal);
        // base16 derived from the M3 roles (mirrors the Go matugenBase16 mapping).
        assert_eq!(v["background"], "#111111"); // surface
        assert_eq!(v["foreground"], "#eeeeee"); // on_surface
        assert_eq!(v["color0"], "#111111");
        assert_eq!(v["color1"], "#ff0000"); // error
        assert_eq!(v["color2"], "#aabbcc"); // primary
        assert_eq!(v["color4"], "#445566"); // secondary
        assert_eq!(v["color8"], "#888888"); // outline
        // camelCase M3 roles carried verbatim.
        assert_eq!(v["surface"], "#111111");
        assert_eq!(v["onSurface"], "#eeeeee");
        assert_eq!(v["primary"], "#aabbcc");
    }

    #[test]
    fn base16_falls_back_when_roles_absent() {
        let v = colors_json(&BTreeMap::new());
        assert_eq!(v["background"], "#121212", "empty palette -> documented fallback");
        assert_eq!(v["foreground"], "#e6e6e6");
    }

    #[test]
    fn flat_palette_reads_default_color() {
        let raw = serde_json::json!({
            "colors": {
                "primary": { "default": { "color": "#123456" } },
                "surface": { "default": { "color": "#abcdef" } },
            }
        });
        let pal = flat_palette(&raw);
        assert_eq!(pal.get("primary").map(String::as_str), Some("#123456"));
        assert_eq!(pal.get("surface").map(String::as_str), Some("#abcdef"));
    }

    #[tokio::test]
    async fn generate_matugen_config_skips_missing_template_files() {
        let tmp = std::env::temp_dir().join(format!("ryogami-test-matugen-{}", std::process::id()));
        let template_dir = tmp.join("templates");
        let cache_dir = tmp.join("cache");
        std::fs::create_dir_all(&template_dir).unwrap();
        std::fs::create_dir_all(&cache_dir).unwrap();

        std::fs::write(template_dir.join("good.conf"), "value = {{colors.primary}}").unwrap();

        let mut config = Config::default();
        config.features.matugen = true;
        config.paths.templates = Some(template_dir.to_string_lossy().to_string());
        config.paths.cache = Some(cache_dir.to_string_lossy().to_string());
        config.integrations = vec![
            Integration {
                name: Some("good".into()),
                template: Some("good.conf".into()),
                output: Some("good-output.conf".into()),
                reload: None,
            },
            Integration {
                name: Some("missing-relative".into()),
                template: Some("does-not-exist.conf".into()),
                output: Some("never-written.conf".into()),
                reload: None,
            },
            Integration {
                name: Some("missing-absolute".into()),
                template: Some("/var/empty/__ryogami_missing__.conf".into()),
                output: Some("also-never.conf".into()),
                reload: None,
            },
            Integration {
                name: Some("missing-tilde".into()),
                template: Some("~/.config/__ryogami_missing_tilde__.conf".into()),
                output: Some("tilde-never.conf".into()),
                reload: None,
            },
        ];

        let cfg_path = generate_matugen_config(&config).await;
        let content = std::fs::read_to_string(&cfg_path).unwrap();

        assert!(content.contains("[templates.good]"), "good integration should be emitted:\n{content}");
        assert!(content.contains("good.conf"), "good template path should be present:\n{content}");

        assert!(!content.contains("[templates.missing-relative]"), "missing-relative should be skipped:\n{content}");
        assert!(!content.contains("does-not-exist.conf"), "missing relative template path should not be emitted:\n{content}");

        assert!(!content.contains("[templates.missing-absolute]"), "missing-absolute should be skipped:\n{content}");
        assert!(!content.contains("__ryogami_missing__.conf"), "missing absolute template path should not be emitted:\n{content}");

        assert!(!content.contains("[templates.missing-tilde]"), "missing-tilde should be skipped:\n{content}");
        assert!(!content.contains("__ryogami_missing_tilde__.conf"), "missing tilde template path should not be emitted:\n{content}");

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[tokio::test]
    async fn generate_matugen_config_emits_only_when_both_template_and_output_set() {
        let tmp = std::env::temp_dir().join(format!("ryogami-test-matugen-pair-{}", std::process::id()));
        let template_dir = tmp.join("templates");
        let cache_dir = tmp.join("cache");
        std::fs::create_dir_all(&template_dir).unwrap();
        std::fs::create_dir_all(&cache_dir).unwrap();

        std::fs::write(template_dir.join("a.conf"), "x").unwrap();
        std::fs::write(template_dir.join("b.conf"), "x").unwrap();

        let mut config = Config::default();
        config.features.matugen = true;
        config.paths.templates = Some(template_dir.to_string_lossy().to_string());
        config.paths.cache = Some(cache_dir.to_string_lossy().to_string());
        config.integrations = vec![
            Integration {
                name: Some("no-output".into()),
                template: Some("a.conf".into()),
                output: None,
                reload: None,
            },
            Integration {
                name: Some("no-template".into()),
                template: None,
                output: Some("orphan.conf".into()),
                reload: None,
            },
            Integration {
                name: Some("complete".into()),
                template: Some("b.conf".into()),
                output: Some("complete.conf".into()),
                reload: None,
            },
        ];

        let cfg_path = generate_matugen_config(&config).await;
        let content = std::fs::read_to_string(&cfg_path).unwrap();

        assert!(content.contains("[templates.complete]"));
        assert!(!content.contains("[templates.no-output]"));
        assert!(!content.contains("[templates.no-template]"));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[tokio::test]
    async fn run_matugen_generates_user_config_templates_end_to_end() {
        let have_matugen = Command::new("matugen")
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !have_matugen {
            eprintln!("matugen not installed; skipping end-to-end test");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(cache.join("wallpaper")).unwrap();

        let img = image::RgbImage::from_fn(64, 64, |x, y| {
            image::Rgb([(x * 4) as u8, (y * 4) as u8, 120])
        });
        let img_path = dir.path().join("wp.png");
        img.save(&img_path).unwrap();

        let user_tmpl = dir.path().join("tmpl.txt");
        std::fs::write(&user_tmpl, "primary={{colors.primary.default.hex}}").unwrap();
        let user_out = dir.path().join("user-out.txt");
        let user_cfg = dir.path().join("user-matugen.toml");
        std::fs::write(
            &user_cfg,
            format!(
                "[config]\n[templates.t]\ninput_path = \"{}\"\noutput_path = \"{}\"\n",
                user_tmpl.display(),
                user_out.display()
            ),
        )
        .unwrap();

        let mut config = Config::default();
        config.features.matugen = true;
        config.paths.cache = Some(cache.to_string_lossy().to_string());
        config.paths.templates = Some(dir.path().to_string_lossy().to_string());
        config.default_matugen_config = Some(user_cfg.to_string_lossy().to_string());
        // Redirect the palette write into the temp dir so it never clobbers the
        // developer's real ~/.cache/ryoku/colors.json.
        let colors = dir.path().join("colors.json");
        config.colors_path_override = Some(colors.clone());

        run_matugen(&img_path.to_string_lossy(), &config).await;

        assert!(
            user_out.exists(),
            "user matugen config template was NOT generated (issue #68)"
        );
        let content = std::fs::read_to_string(&user_out).unwrap();
        assert!(content.starts_with("primary=#"), "template rendered: {content}");

        // Ryogami owns the wallpaper palette: colors.json is authored on apply,
        // carrying the base16 slots and the camelCase Material 3 roles.
        assert!(colors.exists(), "colors.json authored on apply");
        let pal: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(&colors).unwrap()).unwrap();
        for key in ["background", "foreground", "color0", "surface", "primary", "onSurface"] {
            assert!(pal.get(key).and_then(|v| v.as_str()).is_some_and(|s| s.starts_with('#')), "colors.json role {key}");
        }
    }

    #[tokio::test]
    async fn generate_matugen_config_always_has_templates_table_for_matugen4() {
        let tmp = std::env::temp_dir().join(format!("ryogami-test-matugen-empty-{}", std::process::id()));
        let cache_dir = tmp.join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let mut config = Config::default();
        config.features.matugen = true;
        config.paths.cache = Some(cache_dir.to_string_lossy().to_string());
        config.integrations = vec![];

        let cfg_path = generate_matugen_config(&config).await;
        let content = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(
            content.contains("[templates]"),
            "matugen 4.x rejects a config with no templates table (issue #68):\n{content}"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn build_full_palette_extracts_colors_palettes_and_source() {
        let raw = serde_json::json!({
            "is_dark_mode": true,
            "colors": {
                "primary": { "default": { "color": "#aabbcc" } },
                "source_color": { "default": { "color": "#112233" } }
            },
            "palettes": {
                "primary": { "10": { "color": "#0a0a0a" }, "20": { "color": "#141414" } }
            }
        });
        let out = build_full_palette(&raw, "scheme-fidelity", "dark", 0);
        assert_eq!(out["scheme"], "scheme-fidelity");
        assert_eq!(out["is_dark_mode"], true);
        assert_eq!(out["source_color"], "#112233");
        assert_eq!(out["colors"]["primary"], "#aabbcc");
        assert_eq!(out["palettes"]["primary"]["10"], "#0a0a0a");
    }

    fn cache_config_with_current_image() -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.paths.cache = Some(dir.path().to_string_lossy().to_string());
        let wp = dir.path().join("wallpaper");
        std::fs::create_dir_all(&wp).unwrap();
        std::fs::write(wp.join("current.jpg"), b"x").unwrap();
        (dir, config)
    }

    #[tokio::test]
    async fn theme_full_palette_runs_matugen_via_runner_and_parses() {
        let (_dir, config) = cache_config_with_current_image();
        let runner = crate::util::FakeRunner::new();
        runner.on(
            "matugen",
            &["--dry-run"],
            br##"{"colors":{"primary":{"default":{"color":"#abcdef"}}}}"##,
            0,
        );

        let out = theme_full_palette(&runner, &config, "scheme-fidelity", "dark", 2).await.unwrap();
        assert_eq!(out["colors"]["primary"], "#abcdef");
        assert_eq!(out["color_index"], 2);
        assert_eq!(runner.call_count(), 1);
    }

    #[tokio::test]
    async fn theme_full_palette_bails_when_matugen_fails() {
        let (_dir, config) = cache_config_with_current_image();
        let runner = crate::util::FakeRunner::new();
        runner.on("matugen", &["--dry-run"], b"", 1);
        assert!(theme_full_palette(&runner, &config, "s", "dark", 0).await.is_err());
    }

    #[tokio::test]
    async fn theme_full_palette_bails_without_current_image() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.paths.cache = Some(dir.path().to_string_lossy().to_string());
        let runner = crate::util::FakeRunner::new();
        assert!(theme_full_palette(&runner, &config, "s", "dark", 0).await.is_err());
    }
}
