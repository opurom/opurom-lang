# Output path safety and stale artifacts

This round audits the `-o` output path contract. No artifact, emitter or IR
protocol was changed.

## Source basis

Flutter's web server rejects any untrusted request path that can select a
location outside its root and hands the filesystem a normalized path
(`packages/flutter_tools/lib/src/isolated/web_server_utilities.dart:44-56`,
Flutter 3.50.0, commit `ab2912035e160d9cad6306df307576da6128ebdb`). Opurom's
artifact writer always joins the `-o` directory with a generated file name
(`src/emit.rs::write`), so a traversal segment in `-o` is a caller-level path
choice: the parent directory is resolved once and the generated names never
add their own `..` segments.

## Pinned behavior

`tests/output_safety.rs` covers:

- `native | web | wechat | alipay | qq | douyin | baidu | kuaishous | wasm |
  harmony` output through a `sibling` path segment lands in that sibling, and
  no generated artifact name contains `..`;
- repeat emission overwrites every artifact and produces byte-identical trees;
- no `.tmp` or `.bak` file is left in the output directory.

## Stale-route pages: recorded risk

A mini-program route removed from the source keeps its generated page
directory on a re-emit. Flutter's build system deletes previous outputs that
are no longer produced (`packages/flutter_tools/lib/src/build_system/build_system.dart:867-879`),
but Opurom's emitters are additive: they only write the current route set.

This is safe today because `app.json` lists only the current routes and the
framework loader ignores unknown page directories, but it means a stale
directory can survive in a released package. Removing stale route directories
requires an output-manifest contract or a per-target cleanup pass, which is an
artifact-protocol change and is deferred to a coordinated round. The test
documents the current additive behavior rather than changing it.

## Alternatives considered

1. Reject `-o` paths containing `..` outright. Deferred: the parent is
   resolved once and generated names stay inside it, so rejecting the segment
   would remove a legitimate build-directory pattern.
2. Delete the whole output directory before emitting. Rejected: it would
   destroy user-owned files outside the manifest, and the artifact set is not
   currently recorded.
3. Track a per-target manifest and delete missing entries. Deferred: it is an
   output-protocol change and needs a coordinated round.

## Verification

```sh
cargo fmt --all -- --check
cargo test --offline --test output_safety
```
