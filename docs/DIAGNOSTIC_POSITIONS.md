# Diagnostic positions

Diagnostic line and column values are source coordinates: one column per
Unicode scalar value, with columns starting at 1. Dart's `LineInfo` stores line
starts over code-unit offsets and computes `column = offset - lineStart + 1`
(`.pub-cache/hosted/pub.dev/analyzer-14.3.0/lib/source/line_info.dart:78-93`,
SDK 3.14.0, Flutter 3.50.0). It treats `\\r`, `\\n` and `\\r\\n` as line endings
and registers a single start for `\\r\\n` (`source/line_info.dart:45-69`).

Opurom's lexer keeps the same rules over Unicode scalar values:

- `\\r`, `\\n` and `\\r\\n` each advance one line;
- a leading BOM is metadata and does not shift the first column;
- BMP text before an error contributes one column per character;
- a backslash escape advances by the source characters consumed, not by the
  decoded string character count.

`tests/diagnostic_position.rs` pins these rules for unicode prefixes, BOM +
CRLF and bare CR sources. It also rejects byte-offset or escaped-codepoint
text in rendered diagnostics.

## Non-goal

Astral characters (for example emoji) are two UTF-16 code units in Dart but one
Rust `char`. The compiler currently keeps source text and identifiers in BMP
space; adding astral-aware columns would require a coordinated diagnostic
protocol change and is out of scope here.

## Verification

```sh
cargo fmt --all -- --check
cargo test --offline --test diagnostic_position
```
