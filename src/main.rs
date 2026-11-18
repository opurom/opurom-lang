mod catalog;
mod diag;
mod emit;
mod icons;
mod lexer;
mod lower;
mod md_token_roles;
#[cfg(test)]
mod md_token_roles_tests;
mod modules;
mod optimize;
mod parser;
mod pattern;
mod pkg_icons;
mod typecheck;
mod wasm;

use std::fs;
use std::path::Path;

use diag::{DResult, Diagnostic};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const USAGE: &str = "usage: opuromc check <src.opu> | compile <src.opu> -o <out.opuir.json> [--fold-constants] [--fold-nodes] [--no-fold] | target <ir.json> --target <name> -o <dir> | optimize <ir.json> -o <out.json> [--fold-constants] [--full-fold] | targets | components | --version";

fn main() {
    if let Err(e) = run() {
        eprint!("{e}");
        std::process::exit(1);
    }
}

fn read_source(path: &str) -> DResult<String> {
    let bytes = fs::read(path).map_err(|e| {
        Diagnostic::file(
            path,
            format!("cannot read source file: {e}"),
            "check the path, the file encoding (UTF-8) and permissions",
        )
    })?;
    // Dart's UTF-8 decoder discards a leading BOM (`convert/utf.dart:48-49`,
    // SDK 3.14.0, Flutter 3.50.0); malformed bytes are rejected rather than
    // replaced (`convert/utf.dart:33-39`). `Utf8Error::valid_up_to` gives the
    // same zero-based failure offset that `FormatException.offset` records
    // (`core/exceptions.dart:55-66`); convert it to the compiler's 1-based
    // character position so a bad byte late in the file is located precisely.
    let text = String::from_utf8(bytes).map_err(|e| {
        let prefix =
            String::from_utf8_lossy(&e.as_bytes()[..e.utf8_error().valid_up_to()]).into_owned();
        let line = prefix.matches('\n').count()
            + prefix
                .matches('\r')
                .count()
                .saturating_sub(prefix.matches("\r\n").count())
            + 1;
        let col = prefix
            .rsplit(['\n', '\r'])
            .next()
            .map_or(1, |tail| tail.chars().count() + 1);
        Diagnostic::new(
            path,
            line,
            col,
            format!(
                "source is not valid UTF-8 at byte {}",
                e.utf8_error().valid_up_to()
            ),
            "save the source as UTF-8 (malformed bytes are not decoded)",
        )
    })?;
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_string())
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

fn print_targets() {
    // Emitters reported by `opuromc targets`: the TARGETS table plus the
    // responsive DOM build target, kept in declaration order for callers.
    let mut rows: Vec<(&str, &str)> = emit::TARGETS.to_vec();
    rows.insert(2, ("web-dom", "index.html"));
    for (name, artifacts) in rows {
        println!("{name}\t{artifacts}");
    }
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
            let mut ir = lower::compile_source(&src, src_path)?;
            // Constant folding, dead-code elimination and unreachable-node
            // removal run by default. `--no-fold` keeps the raw lowering for
            // conformance checks; the old flags stay accepted as no-ops.
            let idmap = if args.iter().any(|a| a == "--no-fold") {
                None
            } else {
                optimize::fold_default(&mut ir)
            };
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
            if let Some(map) = idmap {
                // Write-only build artifact: nothing in the build or at
                // runtime ever reads this back; it exists for manual and
                // script migration only.
                let sidecar = Path::new(&out).with_extension("idmap.json");
                let text = serde_json::to_string_pretty(&map).map_err(|e| {
                    Diagnostic::file(
                        src_path,
                        format!("cannot serialize id map: {e}"),
                        "report this compiler bug",
                    )
                })? + "\n";
                fs::write(&sidecar, text).map_err(|e| {
                    Diagnostic::file(
                        src_path,
                        format!("cannot write {}: {e}", sidecar.display()),
                        "check the -o directory and permissions",
                    )
                })?;
            }
            Ok(())
        }
        Some("emit")
            if args
                .iter()
                .skip(1)
                .any(|a| matches!(a.as_str(), "--list" | "--help" | "-h" | "help")) =>
        {
            print_targets();
            Ok(())
        }
        Some("target" | "emit") => {
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
            if catalog.components.len() != 260 {
                return Err(Diagnostic::file(
                    "opuromc",
                    format!(
                        "catalog has {} components, expected 260",
                        catalog.components.len()
                    ),
                    "sync the vendored catalog in src/catalog.json with opurom-examples",
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
            optimize::run(
                ir_path,
                Path::new(&out),
                args.iter().any(|a| a == "--fold-constants"),
                args.iter().any(|a| a == "--full-fold"),
            )
        }
        Some("targets") => {
            print_targets();
            Ok(())
        }
        _ => Err(Diagnostic::file(
            "opuromc",
            "missing or unknown subcommand",
            USAGE.to_string(),
        )),
    }
}
