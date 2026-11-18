# Deterministic compile and deferred risk samples

This round adds only tests and documentation. No public IR field, emitter
protocol or source grammar was changed. All commands run inside `opurom-lang`.

## Deterministic compile gate

`tests/compile_env_determinism.rs` compiles the same source three times with:

- declaration order chosen to expose hash-map iteration leakage (`zeta`,
  `alpha`, Unicode `mid`), plus theme, computed and event declarations;
- LF source paths invoked both relatively and canonically;
- unrelated environment perturbation (`OPUROM_HASH_SEED`, `OPR_BASE_URL`,
  `Opurom_FRAME_WIDTH`, `Opurom_FRAME_HEIGHT`).

The three IR byte arrays must be identical and must contain the original CJK
text. The ordering contract follows Dart's JSON encoder example, which shows
authored map member order preserved in output (`convert/json.dart:260-275`,
SDK 3.14.0, Flutter 3.50.0).

## Replayable risk samples

`tests/risk_samples.rs` exposes three CLI-replayable boundaries. None of them
edits a public protocol.

- Depth: a 60-level `Container` chain compiles and emits on `web`; the same
  source at 200 levels compiles but every emitter rejects the IR with
  `recursion limit exceeded`. The parser starts at depth 128
  (`serde_json-1.0.154/src/de.rs:63`) and maps that to the message at
  `serde_json-1.0.154/src/error.rs:384`. See `docs/PAREN_EXPR_RISK.md`.
- Integer: `9007199254740993` survives the IR into generated `web/app.js`.
  Dart specifies 64-bit two's-complement wrapping integers and a narrower
  exact range on JavaScript (`core/int.dart:9-16`, SDK 3.14.0, Flutter 3.50.0),
  so host `Number` still rounds this value at runtime. See
  `docs/INT_PARITY_RISK.md`.
- Parentheses: `(a + 1) * 2` either lowers with the group intact or is rejected
  with a positioned diagnostic; no grammar change is attempted. Dart's AST has
  an explicit `ParenthesizedExpression`
  (`.pub-cache/hosted/pub.dev/analyzer-14.3.0/lib/src/dart/ast/ast.dart:37880-37895`,
  SDK 3.14.0, Flutter 3.50.0). See `docs/PAREN_EXPR_RISK.md`.

## Alternatives kept open

- Depth: keep the supported boundary at 60; or add the `serde_json`
  `unbounded_depth` feature with an explicit recursion guard and stacker. The
  second is a dependency and cross-backend artifact change.
- Integer: staged helper for out-of-safe-range literals; full BigInt for every
  JS-like integer state; or compile-time rejection. Full parity requires the
  second and must be reviewed atomically across emitters.
- Parentheses: keep the existing precedence chain; or add a shared AST node and
  emitter parser update atomically. The latter changes the meaning of existing
  hand-written `computed.expr` strings.

## Verification

```sh
cargo fmt --all -- --check
cargo test --offline --test compile_env_determinism --test risk_samples
```

## Large declaration-order replay

`tests/map_order_stress.rs` compiles 250 states and 250 text bindings three
times and requires byte-identical IR. The case exercises hash-map-backed
validation and dependency paths with many independent keys. Dart's ordered-map
contract rejects unordered iteration as a stable ordering source
(`collection/linked_hash_map.dart:109-115`,
`collection/splay_tree.dart:407-411`, SDK 3.14.0, Flutter 3.50.0).

Verification:

```sh
cargo test --offline --test map_order_stress
```

## List-state mutation boundary

List states are compile-time expansion inputs, not runtime containers. Dart's
unmodifiable list contract rejects length and element changes
(`core/list.dart:239-245`, SDK 3.14.0, Flutter 3.50.0). `opuromc compile` still
records `append`/`removeAt` in IR for source fidelity, but every emitter now
rejects those handlers with an explicit diagnostic. This avoids a silent
mismatch between web, native and mini-program state models without changing
IR fields.

`tests/list_action_boundary.rs` compiles one list mutation and checks the same
failure text on all ten targets.

Verification:

```sh
cargo test --offline --test list_action_boundary
```
