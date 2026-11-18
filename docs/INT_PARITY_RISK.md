# Cross-target integer parity risk note

Scope: JS-like emitters (`web` and six mini-program targets) currently lower
runtime integer updates to host `Number` arithmetic. Dart's `int` is a 64-bit
two's-complement value with wrap-around overflow (`core/int.dart:9-10`, Dart
3.14.0, Flutter 3.50.0). Values above 2^53 lose precision in host `Number`, so
computed values and state action results can differ from the native emitter.

Risk: changing the runtime value representation to BigInt touches every JS-like
emitter, the initial state map, interpolation display, event payload decoding
and all existing code-text assertions. It is a cross-backend protocol change,
not a parameter tweak. Per the standing architecture rule, a written risk
assessment and alternative plan must precede implementation.

## Assessment

- No change to IR fields, no runtime interpreter, no rendering rule change.
- Wire format stays JSON; only generated host code changes.
- Golden IRs remain byte-identical; generated JS text changes only where big
  integer helpers are injected.

## Alternatives

1. Inject a shared i64 helper and keep state values as `number` until an
   out-of-safe-range literal or operation appears. Least churn, but mixed
   representation.
2. Convert all JS-like integer state to BigInt and add an explicit display
   conversion at interpolation boundaries. Uniform semantics, larger change.
3. Keep host `Number` and extend compile-time rejection to every runtime
   integer operation. No runtime change, but rejects valid Dart programs and
   fails the parity bar.

Choice after review: option 2 is required for full parity; option 1 is the
staged path. Task is parked in the risk note until the change is reviewed.

## Follow-up (this round)

No implementation was landed: the risk still stands. A failing red test was
removed from the workspace after the risk was recorded, so the tree stays
green. The next independent deep-round item is deterministic expression
parity for nested logic without adding parentheses to the grammar.
