# Additive map stability

This round audits `stateRefs` and `nodeBoundaries` after import merge, constant
folding and node renumbering. No IR field, emitter protocol or additive-map
schema was changed.

## Source basis

Flutter's depfile reader normalizes paths, filters empty entries and rejects
duplicates before creating a build file set
(`packages/flutter_tools/lib/src/build_system/depfile.dart:112-118`, Flutter
3.50.0, commit `ab2912035e160d9cad6306df307576da6128ebdb`). Opurom applies the
same stability expectation to these additive maps: the incremental pipeline
must be able to compare them across runs.

## Contract

`tests/additive_map_stability.rs` builds a program with two imported states, a
local state, a constant-visibility branch and a constant computed value. It
pins:

- raw `--no-fold` lowering emits neither map;
- default compile emits both maps, with dense node ids `1..n` per tree;
- `stateRefs` values are ordered, deduplicated id lists and the exact expected
  set (`fromA: [1]`, `fromB: [1]`, `local: [1, 2]`, `total: [1]`);
- the root repaint boundary is node `0`, and the text node relayout boundary is
  node `1`;
- `optimize --full-fold` may only remove nodes, so ids stay dense and the map
  shape stays valid;
- a second `--full-fold` pass is a byte no-op.

The maps are deterministic because trees are walked in source order and the
state names and node ids are collected into `BTreeMap` / `BTreeSet` before
serialization.

## Alternatives considered

1. Keep a removed node's id in the map after renumbering. Rejected: a stale id
   would make the incremental pipeline apply the wrong patch.
2. Export the maps from raw lowering. Rejected: raw IR is a conformance
   artifact; the maps belong to the optimized build output.
3. Sort state names alphabetically. Rejected: the current order follows the
   state array, which is itself deterministic and matches the merge order.

## Verification

```sh
cargo fmt --all -- --check
cargo test --offline --test additive_map_stability
```
