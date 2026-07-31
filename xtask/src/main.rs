//! `cargo xtask features` — a thin door to [`rainier_features`].
//!
//! The logic lives in the framework, where the mapping it depends on cannot
//! drift: `rainier-features`' own tests walk every driver enum, so a driver
//! added to Rainier learns its feature in the same commit. This shim exists
//! so the workspace needs no globally installed tool; `cargo install
//! cargo-rainier` gives the same commands anywhere as `cargo rainier …`.
//!
//! ```text
//! cargo xtask features                     # what .env implies, with reasons
//! cargo xtask features --env .env.build
//! cargo xtask features --env .env.build --list   # bare list, for scripts
//! cargo xtask features --check             # CI: fail on an unforwarded selection
//! cargo xtask build --env .env.build --release
//! ```
//!
//! An environment file is **required** — an explicit `--env`, or `.env`.
//! There is no fallback to `.env.example`: sizing a build from the example's
//! defaults would shape the binary like the documentation rather than the
//! deployment, silently. Preview against the defaults with
//! `--env .env.example` when that is what you mean.

use std::path::PathBuf;
use std::process::Command;

use rainier_features::{compute, parse_env, read_sources, resolve_env, Report};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let command = if args.is_empty() { String::new() } else { args.remove(0) };

    let code = match command.as_str() {
        "features" => features_command(&args),
        "build" => build_command(&args),
        _ => {
            eprintln!(
                "usage:\n  cargo xtask features [--env <file>] [--check] [--list]\n  cargo xtask \
                 build [--env <file>] [<cargo args>…]"
            );
            2
        }
    };

    std::process::exit(code);
}

fn features_command(args: &[String]) -> i32 {
    let check = args.iter().any(|a| a == "--check");
    let list = args.iter().any(|a| a == "--list");

    let Some((path, report)) = load(args) else { return 1 };

    if list {
        // The scripting mode: the bare comma-separated list on stdout and
        // nothing else — what the Dockerfile substitutes into `--features`.
        // An unforwarded selection is always fatal here, because a script
        // cannot read a caveat.
        if !report.unforwarded.is_empty() {
            for problem in &report.unforwarded {
                eprintln!("error: {problem}");
            }
            return 1;
        }
        println!("{}", report.feature_list());
        return 0;
    }

    println!("# from {}", path.display());
    for line in &report.reasons {
        println!("#   {line}");
    }
    if report.features.is_empty() {
        println!("# nothing beyond the defaults");
    }
    println!("{}", report.build_command());

    if !report.unforwarded.is_empty() {
        for problem in &report.unforwarded {
            eprintln!("error: {problem}");
        }
        if check {
            return 1;
        }
    }

    0
}

fn build_command(args: &[String]) -> i32 {
    let Some((_, report)) = load(args) else { return 1 };

    if !report.unforwarded.is_empty() {
        for problem in &report.unforwarded {
            eprintln!("error: {problem}");
        }
        return 1;
    }

    let mut cargo = Command::new("cargo");
    cargo.arg("build").arg("--package").arg("app").arg("--no-default-features");

    if !report.features.is_empty() {
        cargo.arg("--features").arg(report.feature_list());
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

fn load(args: &[String]) -> Option<(PathBuf, Report)> {
    let explicit = args
        .iter()
        .position(|a| a == "--env")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);

    let path = match resolve_env(explicit) {
        Ok(path) => path,
        Err(why) => {
            eprintln!("error: {why}");
            return None;
        }
    };

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(why) => {
            eprintln!("could not read {}: {why}", path.display());
            return None;
        }
    };

    let report = compute(&parse_env(&text), &read_sources(std::path::Path::new("src")));
    Some((path, report))
}
