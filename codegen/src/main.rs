mod ast;
mod lower;
mod model;
mod render;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: codegen [check|gen] [--idl <path>]";

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut mode = "gen".to_string();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("cannot locate repository root")?;
    let mut idl = root.join("api/selenium.idl.yaml");
    while let Some(a) = args.next() {
        match a.as_str() {
            "--idl" => idl = PathBuf::from(args.next().ok_or(USAGE)?),
            "gen" | "check" => mode = a,
            _ => return Err(USAGE.into()),
        }
    }

    let text =
        std::fs::read_to_string(&idl).map_err(|e| format!("cannot read {}: {e}", idl.display()))?;
    let doc = model::parse(&text).map_err(|e| format!("{}: {e}", idl.display()))?;
    let abi = lower::lower(&doc).map_err(|e| format!("{}: {e}", idl.display()))?;

    let mut stale = vec![];
    for (path, content) in render::render_all(&abi)? {
        let output = root.join(&path);
        let current = std::fs::read_to_string(&output).ok();
        if current.as_deref() == Some(content.as_str()) {
            continue;
        }
        if mode == "check" {
            stale.push(path.display().to_string());
        } else {
            if let Some(dir) = output.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&output, content).map_err(|e| format!("{}: {e}", output.display()))?;
            println!("generated {}", path.display());
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "generated files are out of date (run `cargo run` from the codegen directory): {}",
            stale.join(", ")
        ))
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
