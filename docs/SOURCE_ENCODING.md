# Source encoding robustness

This round hardens source decoding at the CLI boundary without changing the
IR, emitter protocol or source grammar.

## Behavior

- A leading UTF-8 BOM is discarded. Dart's UTF-8 decoder treats `U+FEFF` as
  BOM metadata and removes it when it appears at the start of input
  (`convert/utf.dart:10-11`, `:48-49`, SDK 3.14.0, Flutter 3.50.0).
- Malformed UTF-8 is rejected with `:1:1: error: source is not valid UTF-8`
  and a help line. Dart's UTF-8 decoder throws a `FormatException` for invalid
  or unterminated sequences unless malformed input is explicitly allowed
  (`convert/utf.dart:33-39`, SDK 3.14.0, Flutter 3.50.0). Opurom does not
  silently replace malformed input with `U+FFFD`.
- Mixed LF, CRLF and CR line endings produce IR byte-identical to the same
  source written with LF only. Dart strings are sequences of UTF-16 code units
  and treat line breaks as ordinary string content (`core/string.dart:9-19`,
  SDK 3.14.0, Flutter 3.50.0); Opurom's lexer consumes all Unicode whitespace
  outside string literals and normalizes lexer positions on line feed.

## Failure safety

`read_source` decodes into memory before lowering. A decode failure returns a
`Diagnostic` and never reaches IR writing, so an existing artifact remains
byte-identical. `tests/source_encoding.rs` asserts this with a good compile
followed by a malformed recompile to the same output path.

## Alternatives considered

1. Keep `fs::read_to_string` and treat BOM as a lexical error. Rejected: it
   diverges from Dart's UTF-8 decoder and breaks sources authored by common
   editors.
2. Replace malformed bytes with `U+FFFD`. Rejected: it silently changes text
   content and violates deterministic source interpretation.
3. Strip BOM and reject malformed input at the CLI boundary. Selected: matches
   Dart's decoder contract while keeping the compiler's IR protocol unchanged.

## Verification

```sh
cargo fmt --all -- --check
cargo test --offline --test source_encoding
```
