# Component expansion depth and cycle boundary

This round adds a compiler robustness guard for recursive component expansion.
No IR field, emitter protocol or component syntax was changed.

## Source basis

Flutter builds an element tree by recursively mounting children; `Element.mount`
requires a parent pointer and a lifecycle transition before a subclass adds its
own children (`packages/flutter/lib/src/widgets/framework.dart:4346-4354`,
Flutter 3.50.0, commit `ab2912035e160d9cad6306df307576da6128ebdb`). Opurom
expands components entirely at compile time, so a cyclic or pathologically deep
declaration graph must not be allowed to exhaust the native stack.

## Contract

`tests/component_expansion.rs` pins:

- self-reference and mutual cycles are rejected with
  `component cycle detected: A -> B -> A` and write no IR;
- an acyclic chain of 200 components expands twice to byte-identical IR and
  passes the literal parameter to the leaf `Text`;
- imported components expand deterministically even when the defining module
  is imported after the module that uses it;
- depth 200 stays inside the guard and depth 1000 fails with
  `component expansion depth exceeds 256`, exit code 1 and no artifact.

The 256-level ceiling is a compiler robustness boundary. It prevents a native
stack abort while leaving ample room for declarative component layering. Long
chains can be split into ordinary container trees or separate built-in nodes
without an IR protocol change.

## Alternatives considered

1. Keep recursion unbounded. Rejected: an acyclic 1000-component chain
   aborted with a native stack overflow before the guard.
2. Add a runtime component interpreter. Rejected: it violates compile-time
   full lowering and would change the IR protocol.
3. Reject cross-module component use. Rejected: imports already define a
   deterministic merge order, and the expansion itself is stable.

## Verification

```sh
cargo fmt --all -- --check
cargo test --offline --test component_expansion
```
