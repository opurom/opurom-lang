# String literal diagnostics

This round pins the lexer boundary for unterminated single-line string
literals. No IR, emitter protocol or grammar change is included.

## Source basis

Dart distinguishes single-line and multiline string literals:

- `singleLineStringLiteral` uses matching single or double quotes.
- `multiLineStringLiteral` uses `'''` or `"""` and may span lines.

See `.pub-cache/hosted/pub.dev/analyzer-14.3.0/lib/src/dart/ast/ast.dart:46492-46507`
and `bin/cache/dart-sdk/lib/core/string.dart:17-34`, SDK 3.14.0,
Flutter 3.50.0. Opurom accepts only the double-quoted single-line form.

## Pinned behavior

- An unclosed quote before a line feed reports the opening quote position and
  a help line ending with `before the end of the line`.
- An unclosed quote at EOF reports the opening quote position and a help line
  ending with `before the end of the file`.
- A trailing backslash at EOF is not treated as a closed string.
- `\"` and `\\` are valid escapes and do not close a string early.
- A rejected source never writes a new IR and never overwrites a previous IR.

The opening-position rule keeps the diagnostic independent of how much text
follows the quote. It also matches the token position stored by the lexer and
used by the existing positioned-diagnostic gate.

## Alternatives considered

1. Accept bare newlines inside strings. Rejected: it changes the source grammar
   and blurs the boundary between single-line and triple-quoted strings.
2. Add triple-quoted strings. Deferred: it is a coordinated grammar and emitter
   change, not a lexer diagnostic fix.
3. Report the EOF or newline position instead of the opening quote. Rejected:
   the caret would point at the place where the string stopped, not at the
   token that must be fixed.

## Verification

```sh
cargo fmt --all -- --check
cargo test --offline --test unterminated_string
```
