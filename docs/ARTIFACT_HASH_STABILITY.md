# Cross-target artifact hash stability

This round adds a release-manifest gate for emitted files. No emitter protocol
or IR field was changed.

## Source basis

Flutter's depfile reader normalizes output paths, filters empty entries and
removes duplicates before creating the build file set
(`packages/flutter_tools/lib/src/build_system/depfile.dart:112-118`, Flutter
3.50.0, commit `ab2912035e160d9cad6306df307576da6128ebdb`). Flutter's build
hash data is documented as reproducible and noise-free
(`packages/flutter_tools/lib/src/build_system/hash.dart:7-9`). Opurom treats
`opuromc targets` as the equivalent declared artifact manifest.

## Gate

`tests/artifact_hash_stability.rs` checks the ten built-in targets (excluding
the sibling-repo `web-dom` emitter):

- the manifest has one row per target and every declared path exists;
- the emitted recursive file-name set exactly equals the declared set;
- every declared file is non-empty;
- two emissions from the same IR are byte-identical;
- a third emission reproduces a dependency-free FNV-1a hash for each file;
- emitting with an absolute IR path and a relative IR path yields identical
  artifact bytes.

The hash is a change detector inside the test, not a new compiler artifact or
public protocol. A path, file-set, empty-file or byte change fails CI even if
the target still exits successfully.

## Alternatives considered

1. Trust the emitter exit code. Rejected: an emitter can succeed while
   silently dropping a platform config file.
2. Compare only file bytes, not the name set. Rejected: a renamed manifest
   entry would break packaging even with valid content.
3. Emit a compiler-owned manifest hash into IR. Deferred: it changes the IR
   contract and is unnecessary for this gate.

## Verification

```sh
cargo fmt --all -- --check
cargo test --offline --test artifact_hash_stability
```
