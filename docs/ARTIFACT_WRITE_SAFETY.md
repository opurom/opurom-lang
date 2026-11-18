# Artifact write safety

`compile` lowers in memory and writes only after a successful pass. `optimize`
now follows the same failure-safety contract for in-place calls: it writes to
`<output>.tmp` and renames it over the output only after serialization and all
validation steps have succeeded. A failed validation leaves the previous IR
byte-identical and no temporary file.

The CLI output path itself is not part of the IR, so the temporary file never
touches compiler semantics or emitted target code. Flutter's build pipeline
similarly treats outputs as replaceable artifacts: `devfs.dart` uploads a
complete compiled payload only after the compiler has produced it, and a
failed compile keeps the previous loaded state
(`packages/flutter_tools/lib/src/devfs.dart:636-642`, Flutter 3.50.0,
commit `ab2912035e160d9cad6306df307576da6128ebdb`).

`tests/optimize_failure_safety.rs` covers an in-place optimize of invalid IR
and asserts:

- exit code 1 with a positioned IR diagnostic;
- the original bytes are preserved;
- no `.tmp` file is left behind.

## Verification

```sh
cargo fmt --all -- --check
cargo test --offline --test optimize_failure_safety
```

## Cross-target action chain replay

Interactive correctness uses a deterministic action chain compiled once and
emitted ten times: `a += b`, `b += a`, `a += 7`. The source order changes
`a` twice and must be preserved by every target. The IR stores the chain as an
ordered `actions` array; Flutter's frontend server likewise returns an ordered
compile result with an explicit error count before a bundle is loaded
(`packages/flutter_tools/lib/src/compile.dart:96-114`,
`packages/flutter_tools/lib/src/devfs.dart:636-642`, Flutter 3.50.0,
commit `ab2912035e160d9cad6306df307576da6128ebdb`).

`tests/action_chain.rs` checks the IR order and exact generated statement order
for web, native, wechat and alipay, and checks that all ten targets emit the
chain without failure.

Verification:

```sh
cargo test --offline --test action_chain
```
