# Raw and optimized IR emitter equivalence

This round adds a semantic-marker gate for raw versus optimized emitted
sources. No emitter, IR or runtime protocol was changed.

## Source basis

Flutter's depfile reader normalizes paths, filters empty entries and rejects
duplicates before creating the build file set
(`packages/flutter_tools/lib/src/build_system/depfile.dart:112-118`, Flutter
3.50.0, commit `ab2912035e160d9cad6306df307576da6128ebdb`). Opurom treats the
raw IR and the optimized IR as two build inputs that must describe the same
program even when their bytes differ.

## Gate

`tests/emit_equivalence.rs` compiles one source twice:

- `--no-fold` keeps the raw expression `2 + 3 * 4` and interpolated text;
- the default compile path folds the expression to `14` and the visible text to
  `constant: 14 / true`.

It then emits both IRs on all ten built-in targets and asserts:

- each target succeeds for both IR forms;
- the emitted artifact name set is unchanged by folding;
- every generated source tree still contains the markers `14`, `constant`,
  `true`, `hidden` and `count`.

The marker check is a harness-side semantic comparison, not a compiler feature:
it checks that the folded values and the mutable/hidden branches still reach
generated code. Byte equality is deliberately not required because folding
legitimately rewrites expressions and text. This avoids adding a runtime
interpreter or changing the emitter protocol.

## Alternatives considered

1. Require byte-identical emitted sources. Rejected: it would forbid the
   optimization the gate is meant to audit.
2. Execute all ten target artifacts and compare stdout. Deferred: it needs
   target-specific runtimes and a shared rendering protocol across repos.
3. Add an IR provenance map to emitted code. Rejected: it changes the IR and
   artifact schema for a test-only concern.

## Verification

```sh
cargo fmt --all -- --check
cargo test --offline --test emit_equivalence
```
