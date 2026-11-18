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

## Overflow audit (this round)

`tests/integer_overflow_audit.rs` pins the current boundaries without changing
runtime representation:

- compile-time computed expressions reject `+`, `-` and `*` overflow with the
  exact `i64` range in the help line and write no IR;
- integer literals outside `i64` are rejected;
- list indexes and delays are bounded before `usize` conversion or scheduling;
- an `i64::MAX` state plus an action chain emits on all ten targets, with the
  native emitter keeping the `i64` state and its `saturating_add` path.

Dart specifies 64-bit two's-complement wrap-around for the default `int`
(`core/int.dart:9-10`, SDK 3.14.0, Flutter 3.50.0). Opurom therefore does **not**
yet have full parity: compile-time overflow is rejected rather than wrapped,
and native runtime actions saturate rather than wrap. JS-like targets still
emit the literal into host `Number`, which rounds values above 2^53
(`core/int.dart:12-16`). Changing these behaviors atomically would touch
computed-expression evaluation, every emitter's action code and the BigInt
representation decision, so this round records the divergence and adds tests
only.

The open choices remain the three alternatives listed above. Full parity still
requires option 2 (uniform BigInt with explicit display conversion) plus a
matching decision for runtime action overflow; the staged helper remains the
lower-risk path.
