# Cross-module import merge boundary

This round audits the existing import merge rules. No IR field, emitter
protocol or import grammar was changed.

## Source basis

Flutter's depfile parser deduplicates collected paths before creating a file
list (`packages/flutter_tools/lib/src/build_system/depfile.dart:116-118`,
Flutter 3.50.0, commit `ab2912035e160d9cad6306df307576da6128ebdb`). Opurom
applies the same no-duplicate principle to declarations: a module path may be
imported twice, but a declaration name may have only one source.

## Merge contract

`tests/import_merge.rs` creates self-contained projects with an `opurom.json`
module root and pins:

- states, models and APIs from modules follow import order; main-program
  states are appended last;
- repeated import of the same module is an idempotent no-op;
- a state/model/API name declared by two modules, or by a module and the main
  program, fails with `duplicate imported name '<name>'` and no IR;
- a duplicate model stops the merge before the later API and component
  comparisons, so the first conflict is the stable diagnostic;
- components merge in import order after main components and also reject
  duplicates;
- a theme may come from at most one source: two module themes, or a module
  theme plus a main theme, fail with `duplicate imported name 'theme'`.

No protocol change is required. The rules are deterministic because the merge
walks imports depth-first in source order and keeps one owner per name.

## Alternatives considered

1. Last-wins override for duplicate names. Rejected: it would make IR depend
   silently on import order and hide conflicting module contracts.
2. Error on every duplicate in one pass. Deferred: the current fail-fast merge
   reports the first conflict at its source position, which is already stable
   and avoids cascaded diagnostics.
3. Deduplicate declarations by value rather than name. Rejected: two models
   with identical fields still have distinct public names.

## Verification

```sh
cargo fmt --all -- --check
cargo test --offline --test import_merge
```
