//! Client mode: `ryogami <verb> …` forwards one line command to the running
//! daemon on `ryogami.sock` and prints its reply. With no verb (or `daemon`) the
//! binary runs the daemon instead (see `main`). The wire grammar is the CLI
//! grammar verbatim, so the tokens join into the command with single spaces.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// The verbs the CLI forwards to the daemon (`wallpaper …`, `depth …`); every
/// other first token (empty, `daemon`, flags) means "run the daemon".
const VERBS: [&str; 2] = ["wallpaper", "depth"];

/// Map argv (after the program name) to the wire command line, or `None` when the
/// args are not a client command and the binary should run the daemon.
#[must_use]
pub fn command_line(args: &[String]) -> Option<String> {
    let first = args.first()?;
    if VERBS.contains(&first.as_str()) {
        Some(args.join(" "))
    } else {
        None
    }
}

/// Send one command line to the daemon and print its single-line reply. Exits the
/// process with status 1 when the daemon replies `err …` (or is unreachable), so
/// a keybind / script sees a real failure.
pub async fn run(line: &str) -> anyhow::Result<()> {
    let sock = ryogami_proto::socket_path();
    let stream = UnixStream::connect(&sock)
        .await
        .map_err(|e| anyhow::anyhow!("cannot reach ryogami at {}: {e}", sock.display()))?;
    let (reader, mut writer) = stream.into_split();
    writer.write_all(line.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;

    let mut reader = BufReader::new(reader);
    let mut reply = String::new();
    reader.read_line(&mut reply).await?;
    let reply = reply.trim_end();
    if !reply.is_empty() {
        println!("{reply}");
    }
    if reply.starts_with("err") {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn maps_wallpaper_set_to_line_command() {
        assert_eq!(command_line(&argv(&["wallpaper", "set", "x"])).as_deref(), Some("wallpaper set x"));
    }

    #[test]
    fn preserves_flags_and_spaced_paths_verbatim() {
        assert_eq!(
            command_line(&argv(&["wallpaper", "set", "/a b.png", "--screen", "DP-1"])).as_deref(),
            Some("wallpaper set /a b.png --screen DP-1"),
        );
        assert_eq!(command_line(&argv(&["wallpaper", "random", "start"])).as_deref(), Some("wallpaper random start"));
        assert_eq!(command_line(&argv(&["wallpaper", "resource", "high"])).as_deref(), Some("wallpaper resource high"));
        assert_eq!(command_line(&argv(&["depth", "refresh"])).as_deref(), Some("depth refresh"));
    }

    #[test]
    fn maps_new_wallpaper_modes_to_line_commands() {
        // The three modes added for the ryogami cutover forward verbatim, incl. the
        // hyphen in `live-reload` and an optional per-output `--screen`.
        assert_eq!(command_line(&argv(&["wallpaper", "next"])).as_deref(), Some("wallpaper next"));
        assert_eq!(command_line(&argv(&["wallpaper", "repaint"])).as_deref(), Some("wallpaper repaint"));
        assert_eq!(command_line(&argv(&["wallpaper", "live-reload"])).as_deref(), Some("wallpaper live-reload"));
        assert_eq!(
            command_line(&argv(&["wallpaper", "next", "--screen", "DP-1"])).as_deref(),
            Some("wallpaper next --screen DP-1"),
        );
    }

    #[test]
    fn no_verb_or_daemon_runs_the_daemon() {
        assert_eq!(command_line(&[]), None, "no args -> daemon");
        assert_eq!(command_line(&argv(&["daemon"])), None, "explicit daemon");
        assert_eq!(command_line(&argv(&["--flag"])), None, "unknown first token -> daemon");
    }
}
