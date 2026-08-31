fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    emit_version();

    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("linux") {
        return;
    }
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/ryogami:$ORIGIN");
}

fn emit_version() {
    let version = git_version()
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".into()));
    println!("cargo:rustc-env=RYOGAMI_VERSION={version}");
    for p in ["../../.git/HEAD", "../../.git/index"] {
        if std::path::Path::new(p).exists() {
            println!("cargo:rerun-if-changed={p}");
        }
    }
}

fn git_version() -> Option<String> {
    let run = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git").args(args).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    };
    let count = run(&["rev-list", "--count", "HEAD"])?;
    let hash = run(&["rev-parse", "--short", "HEAD"])?;
    Some(format!("r{count}.{hash}"))
}
