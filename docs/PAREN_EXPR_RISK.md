# Parenthesized computed expressions

Dart models parentheses explicitly with `ParenthesizedExpression`
(`.pub-cache/hosted/pub.dev/analyzer-14.3.0/lib/src/dart/ast/ast.dart:37880-37895`,
SDK 3.14.0, Flutter 3.50.0).
Opurom's `computed.expr` is a canonical token string re-parsed by
`emit::computed_expr` in every emitter. It currently supports `+ - *`,
comparisons, `&& || !`, literals and state names, but rejects `(`.

Adding `(` `)` changes the source grammar, the AST, `Expr::text`, operator
precedence tests, the shared emitter parser, and the optimizer liveness scanner.
It does not add an IR field, but it changes the meaning of the existing IR
`expr` string for hand-written IR and must land atomically across all emitters.

This is a cross-module language change. It is recorded here as a risk note
before implementation; no code change is included in this round.

## Follow-up (this round)

No parser or emitter change was made. The existing precedence chain
(`or -> and -> cmp -> sum -> term -> factor`) already gives deterministic
grouping for expressions that avoid parentheses; the missing syntax is
recorded for a coordinated language round.

## IR recursion boundary (observed)

`opuromc compile` lowers a 200-level container chain, but every emitter reads
IR through `serde_json::Value` with its default 128-level recursion limit
(`serde_json-1.0.154/src/de.rs:63`, `src/error.rs:384`), so the artifact cannot
be consumed. `tests/risk_samples.rs` replays both sides: 60 levels emit, 200
levels compile but fail at emit with `recursion limit exceeded`. A bounded parser limit is not exposed by
`serde_json` 1.0.154; lifting it safely requires the `unbounded_depth` feature
plus an explicit stacker, which is a crate/dependency change. Until then the
supported depth is <= 60 and the regression test pins that boundary.
