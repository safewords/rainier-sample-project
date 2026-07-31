//! `cargo xtask features` — the feature set a deployment actually needs.
//!
//! # Why this exists
//!
//! Cargo cannot enable features from code. They are resolved **before**
//! anything compiles, they are additive-only, and a build script cannot add
//! one — so "the compiler notices you selected `MAIL_DRIVER=smtp` and turns
//! `mail-smtp` on" is not a thing cargo can do, however nice it sounds.
//! Nor does dead-code elimination save the binary on its own: the driver
//! `match`es in `bootstrap.rs` and the providers are deliberately
//! exhaustive, so every compiled driver is *referenced* and the linker keeps
//! it. Features are the mechanism that sizes the binary, and something has
//! to compute them.
//!
//! This is that something. It reads the two honest sources —
//!
//! 1. **the deployment's environment file**, where every driver selection
//!    lives (`CACHE_DRIVER`, `QUEUE_DRIVER`, `MAIL_DRIVER`, `HASH_DRIVER`,
//!    `STORAGE_DRIVER`, …), and
//! 2. **the source tree**, for the APIs that are compile-time choices rather
//!    than runtime selections (`Jwt`, the `Http` facade's real transport),
//!
//! — and prints the minimal `--features` list, or runs the build with it:
//!
//! ```text
//! cargo xtask features                     # what .env implies, and the command
//! cargo xtask features --env .env.production
//! cargo xtask features --check             # CI: fail on a selection nothing forwards
//! cargo xtask build --env .env.production --release
//! ```
//!
//! The mapping table below is the contract. When the framework grows a
//! driver, the compiler already points at the `match` arms that must learn
//! it; add its row here too, and `--check` keeps deployments from selecting
//! things no feature forwards.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let rest: Vec<String> = args.collect();

    let code = match command.as_str() {
        "features" => features_command(&rest),
        "build" => build_command(&rest),
        _ => {
            eprintln!(
                "usage:\n  cargo xtask features [--env <file>] [--check]\n  cargo xtask build \
                 [--env <file>] [<cargo args>…]"
            );
            2
        }
    };

    std::process::exit(code);
}

/// `cargo xtask features`.
fn features_command(args: &[String]) -> i32 {
    let check = args.iter().any(|a| a == "--check");
    let env_path = flag_value(args, "--env").unwrap_or_else(default_env_file);

    let env = match parse_env_file(&env_path) {
        Ok(env) => env,
        Err(why) => {
            eprintln!("could not read {}: {why}", env_path.display());
            return 1;
        }
    };

    let sources = read_sources(Path::new("src"));
    let report = compute(&env, &sources);

    println!("# from {}", env_path.display());
    for line in &report.reasons {
        println!("#   {line}");
    }

    if report.features.is_empty() {
        println!("# nothing beyond the defaults");
        println!("cargo build --release --no-default-features");
    } else {
        let list: Vec<&str> = report.features.iter().map(String::as_str).collect();
        println!(
            "cargo build --release --no-default-features --features \"{}\"",
            list.join(",")
        );
    }

    if !report.unmapped.is_empty() {
        for problem in &report.unmapped {
            eprintln!("error: {problem}");
        }
        if check {
            return 1;
        }
    }

    0
}

/// `cargo xtask build` — compute, then hand the rest to cargo.
fn build_command(args: &[String]) -> i32 {
    let env_path = flag_value(args, "--env").unwrap_or_else(default_env_file);

    let env = match parse_env_file(&env_path) {
        Ok(env) => env,
        Err(why) => {
            eprintln!("could not read {}: {why}", env_path.display());
            return 1;
        }
    };

    let report = compute(&env, &read_sources(Path::new("src")));

    if !report.unmapped.is_empty() {
        for problem in &report.unmapped {
            eprintln!("error: {problem}");
        }
        return 1;
    }

    let mut cargo = Command::new("cargo");
    cargo.arg("build").arg("--no-default-features");

    if !report.features.is_empty() {
        let list: Vec<&str> = report.features.iter().map(String::as_str).collect();
        cargo.arg("--features").arg(list.join(","));
    }

    // Everything except the `--env <file>` pair goes to cargo untouched.
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "--env" {
            skip_next = true;
            continue;
        }
        cargo.arg(arg);
    }

    match cargo.status() {
        Ok(status) if status.success() => 0,
        Ok(_) => 1,
        Err(why) => {
            eprintln!("could not run cargo: {why}");
            1
        }
    }
}

/// What was computed, and why.
struct Report {
    features: BTreeSet<String>,
    reasons: Vec<String>,
    unmapped: Vec<String>,
}

/// The mapping — the whole tool, really.
fn compute(env: &[(String, String)], sources: &str) -> Report {
    let mut features = BTreeSet::new();
    let mut reasons = Vec::new();
    let mut unmapped = Vec::new();

    let get = |name: &str| -> Option<&str> {
        env.iter().rev().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    };

    let mut want = |feature: &str, why: String, reasons: &mut Vec<String>| {
        if features.insert(feature.to_string()) {
            reasons.push(format!("{feature:<14} {why}"));
        }
    };

    // --- runtime selections, from the environment ---------------------------

    for source in ["CACHE_DRIVER", "SESSION_DRIVER"] {
        match get(source) {
            Some("redis") => want("redis", format!("{source}=redis"), &mut reasons),
            Some("redis-cluster") => {
                want("redis-cluster", format!("{source}=redis-cluster"), &mut reasons)
            }
            Some("memcached") => want("memcached", format!("{source}=memcached"), &mut reasons),
            Some("dynamodb" | "kv") => unmapped.push(format!(
                "{source}={} is not wired in this application — see `bootstrap::cache`",
                get(source).unwrap_or_default()
            )),
            _ => {}
        }
    }

    match get("QUEUE_DRIVER") {
        Some("redis") => want("redis", "QUEUE_DRIVER=redis".into(), &mut reasons),
        Some("sqs") => want("sqs", "QUEUE_DRIVER=sqs".into(), &mut reasons),
        Some("kafka") => {
            want("kafka", "QUEUE_DRIVER=kafka".into(), &mut reasons);
            if get("KAFKA_TLS").is_some_and(|tls| tls == "true" || tls == "1") {
                want("kafka-tls", "KAFKA_TLS=true".into(), &mut reasons);
            }
        }
        _ => {}
    }

    match get("MAIL_DRIVER") {
        Some(sender @ ("smtp" | "ses" | "postmark" | "mailgun" | "sendgrid" | "resend")) => {
            let feature = format!("mail-{sender}");
            want(&feature, format!("MAIL_DRIVER={sender}"), &mut reasons);
        }
        _ => {}
    }

    if get("HASH_DRIVER") == Some("bcrypt") {
        want("bcrypt", "HASH_DRIVER=bcrypt".into(), &mut reasons);
    }

    if get("STORAGE_DRIVER") == Some("s3") {
        want("s3", "STORAGE_DRIVER=s3".into(), &mut reasons);
    }

    // --- compile-time choices, from the source -------------------------------
    //
    // These are not selected by a variable: code either reaches for the API
    // or it does not. Substring matches are crude and cheap, and a false
    // positive costs one feature, not a broken build.

    if sources.contains("BcryptVerifier") || sources.contains("BcryptHasher") {
        want("bcrypt", "src/ names a bcrypt type".into(), &mut reasons);
    }

    if sources.contains("crypt::Jwt")
        || sources.contains("JwtKeyRing")
        || sources.contains("JwtKey::")
    {
        want("jwt", "src/ names the JWT surface".into(), &mut reasons);
    }

    if sources.contains("Http::") || sources.contains("ReqwestTransport") {
        want("http-client", "src/ uses the Http facade".into(), &mut reasons);
    }

    Report { features, reasons, unmapped }
}

/// `--flag value` out of an argument list.
fn flag_value(args: &[String], flag: &str) -> Option<PathBuf> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(PathBuf::from)
}

/// `.env` when it exists, `.env.example` otherwise.
fn default_env_file() -> PathBuf {
    let dot_env = PathBuf::from(".env");
    if dot_env.exists() {
        dot_env
    } else {
        PathBuf::from(".env.example")
    }
}

/// `KEY=VALUE` lines, in order — later lines win, like a shell.
fn parse_env_file(path: &Path) -> Result<Vec<(String, String)>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(parse_env(&text))
}

fn parse_env(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            let value = value.trim().trim_matches('"').trim_matches('\'');
            Some((key.trim().to_string(), value.to_string()))
        })
        .collect()
}

/// Every `.rs` under `root`, concatenated. Comments are not stripped — a
/// commented-out `Http::` costs a feature, which is the cheap direction to
/// be wrong in.
fn read_sources(root: &Path) -> String {
    let mut out = String::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push_str(&text);
                    out.push('\n');
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(text: &str) -> Vec<(String, String)> {
        parse_env(text)
    }

    #[test]
    fn a_fresh_clone_needs_nothing() {
        let report = compute(&env("CACHE_DRIVER=memory\nQUEUE_DRIVER=sync"), "");

        assert!(report.features.is_empty(), "{:?}", report.features);
        assert!(report.unmapped.is_empty());
    }

    #[test]
    fn every_driver_selection_maps_to_its_feature() {
        let report = compute(
            &env("CACHE_DRIVER=redis\nQUEUE_DRIVER=sqs\nMAIL_DRIVER=smtp\nHASH_DRIVER=bcrypt\nSTORAGE_DRIVER=s3"),
            "",
        );

        let expected: BTreeSet<String> =
            ["redis", "sqs", "mail-smtp", "bcrypt", "s3"].iter().map(|s| s.to_string()).collect();
        assert_eq!(report.features, expected);
    }

    #[test]
    fn kafka_brings_tls_only_when_the_cluster_asks() {
        let plain = compute(&env("QUEUE_DRIVER=kafka"), "");
        assert!(plain.features.contains("kafka"));
        assert!(!plain.features.contains("kafka-tls"));

        let tls = compute(&env("QUEUE_DRIVER=kafka\nKAFKA_TLS=true"), "");
        assert!(tls.features.contains("kafka-tls"));
    }

    #[test]
    fn source_usage_is_a_reason_too() {
        let report = compute(&[], "let jwt = crypt::Jwt::new(ring);\nHttp::get(url)");

        assert!(report.features.contains("jwt"));
        assert!(report.features.contains("http-client"));
    }

    #[test]
    fn a_selection_nothing_forwards_is_an_error_rather_than_a_silence() {
        // The failure --check exists for: a deployment asks for a driver this
        // application never wired, and the answer must not be a feature list
        // that quietly omits it.
        let report = compute(&env("CACHE_DRIVER=dynamodb"), "");

        assert!(!report.unmapped.is_empty());
    }

    #[test]
    fn later_lines_win_like_a_shell() {
        let report = compute(&env("MAIL_DRIVER=smtp\nMAIL_DRIVER=log"), "");

        assert!(report.features.is_empty(), "{:?}", report.features);
    }

    #[test]
    fn quotes_and_comments_are_not_values() {
        let parsed = env("# comment\nMAIL_DRIVER=\"smtp\"\n\nEMPTY=");

        assert!(parsed.contains(&("MAIL_DRIVER".to_string(), "smtp".to_string())));
        assert!(parsed.contains(&("EMPTY".to_string(), String::new())));
    }
}
