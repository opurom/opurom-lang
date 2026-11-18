# Interpolation stress boundary

This round adds tests only. No IR field, emitter protocol or interpolation
grammar was changed.

## Source basis

Dart models interpolation as alternating literal and expression elements;
`InterpolationExpression` covers a simple identifier or a braced expression
(`.pub-cache/hosted/pub.dev/analyzer-14.3.0/lib/src/dart/ast/ast.dart:29439-29469`,
SDK 3.14.0, Flutter 3.50.0). Opurom keeps the same linear model in
`lower::interp_parts`: `{{` and `}}` are literal braces, `{name}` is a scalar
state read, and `{items[i]}` is a compile-time list read.

## Pinned behavior

`tests/interpolation_stress.rs` covers:

- a 20,000-character literal plus 500 placeholders compiled twice to
  byte-identical IR, with placeholders folded to the declared state value;
- 5,000 repeated `{{x}}` pairs preserved as literal text;
- a 10,000-character prefix before an unterminated `{` reported at
  `1:10021` with the standard brace-escape help and no IR;
- 5,000 valid placeholders followed by an unknown state reported at
  `3:15010` with a positioned diagnostic and no IR.

The position is a source character column, computed over the decoded Unicode
string. This matches the compiler's diagnostic contract and avoids leaking
UTF-8 byte offsets into user-facing messages.

## Alternatives considered

1. Cap interpolation parts. Deferred: it would reject currently valid large
   templates and change the source contract.
2. Parse nested braces into full Dart-style expressions. Deferred: it changes
   the IR `expr` meaning and all emitters.
3. Keep linear parsing and pin diagnostics plus determinism. Selected: no
   protocol change and no silent truncation.

## Verification

```sh
cargo fmt --all -- --check
cargo test --offline --test interpolation_stress
```
