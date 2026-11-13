mod catalog;
mod diag;
mod emit;
mod lexer;
mod lower;
mod modules;
mod optimize;
mod parser;
mod typecheck;
mod wasm;

use std::fs;
use std::path::Path;

use diag::{DResult, Diagnostic};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const USAGE: &str = "usage: opuromc check <src.opu> | compile <src.opu> -o <out.opuir.json> | target <ir.json> --target <name> -o <dir> | optimize <ir.json> -o <out.json> | targets | components | --version";

fn main() {
    if let Err(e) = run() {
        eprint!("{e}");
        std::process::exit(1);
    }
}

fn read_source(path: &str) -> DResult<String> {
    fs::read_to_string(path).map_err(|e| {
        Diagnostic::file(
            path,
            format!("cannot read source file: {e}"),
            "check the path, the file encoding (UTF-8) and permissions",
        )
    })
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn need<'a>(args: &'a [String], index: usize, what: &str) -> DResult<&'a str> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| Diagnostic::file("opuromc", format!("missing {what}"), USAGE.to_string()))
}

fn run() -> DResult<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => {
            println!("opuromc {VERSION}");
            Ok(())
        }
        Some("check") => {
            let src_path = need(&args, 1, "source file for 'check'")?;
            let src = read_source(src_path)?;
            let _ = lower::compile_source(&src, src_path)?;
            Ok(())
        }
        Some("compile") => {
            let src_path = need(&args, 1, "source file for 'compile'")?;
            let out = flag_value(&args[2..], "-o").ok_or_else(|| {
                Diagnostic::file(
                    "opuromc",
                    "compile needs '-o <out.opuir.json>'",
                    USAGE.to_string(),
                )
            })?;
            let src = read_source(src_path)?;
            let ir = lower::compile_source(&src, src_path)?;
            let text = serde_json::to_string_pretty(&ir).map_err(|e| {
                Diagnostic::file(
                    src_path,
                    format!("cannot serialize IR: {e}"),
                    "report this compiler bug",
                )
            })? + "\n";
            fs::write(&out, text).map_err(|e| {
                Diagnostic::file(
                    src_path,
                    format!("cannot write {out}: {e}"),
                    "check the -o path and permissions",
                )
            })?;
            Ok(())
        }
        Some("target") => {
            let ir_path = need(&args, 1, "IR file for 'target'")?;
            let target = flag_value(&args[2..], "--target").ok_or_else(|| {
                Diagnostic::file(
                    "opuromc",
                    "target needs '--target <name>' (try 'opuromc targets')",
                    USAGE.to_string(),
                )
            })?;
            let out = flag_value(&args[2..], "-o").ok_or_else(|| {
                Diagnostic::file("opuromc", "target needs '-o <dir>'", USAGE.to_string())
            })?;
            let text = fs::read_to_string(ir_path).map_err(|e| {
                Diagnostic::file(
                    ir_path,
                    format!("cannot read IR file: {e}"),
                    "compile a .opu file with 'opuromc compile' first",
                )
            })?;
            emit::emit(&target, &text, ir_path, Path::new(&out))
        }
        Some("components") => {
            let catalog = catalog::catalog()?;
            if catalog.components.len() != 216 {
                return Err(Diagnostic::file(
                    "opuromc",
                    format!(
                        "catalog has {} components, expected 216",
                        catalog.components.len()
                    ),
                    "sync the vendored catalog in src/catalog.json with opurom-spec",
                ));
            }
            for c in &catalog.components {
                println!("{} | {} | {}", c.name, c.primitive, c.category);
            }
            println!("{} components", catalog.components.len());
            Ok(())
        }
        Some("optimize") => {
            let ir_path = need(&args, 1, "IR file for 'optimize'")?;
            let out = flag_value(&args[2..], "-o").ok_or_else(|| {
                Diagnostic::file(
                    "opuromc",
                    "optimize needs '-o <out.json>'",
                    USAGE.to_string(),
                )
            })?;
            optimize::run(ir_path, Path::new(&out))
        }
        Some("targets") => {
            for (name, artifacts) in emit::TARGETS {
                println!("{name}\t{artifacts}");
            }
            Ok(())
        }
        _ => Err(Diagnostic::file(
            "opuromc",
            "missing or unknown subcommand",
            USAGE.to_string(),
        )),
    }
}
