# Parenthesized computed expressions

Dart models parentheses explicitly with `ParenthesizedExpression`
(`pkg/analyzer/lib/src/dart/ast/ast.dart:40397-40410`, SDK 3.14.0, Flutter 3.50.0).
Opurom's `computed.expr` is a canonical token string re-parsed by
`emit::computed_expr` in every emitter. It currently supports `+ - *`,
comparisons, `&& || !`, literals and state names, but rejects `(`.

Adding `(` `)` changes the source grammar, the AST, `Expr::text`, operator
precedence tests, the shared emitter parser, and the optimizer liveness scanner.
It does not add an IR field, but it changes the meaning of the existing IR
`expr` string for hand-written IR and must land atomically across all emitters.

This is a cross-module language change. It is recorded here as a risk note
before implementation; no code change is included in this round.

## Follow-up (this round)

No parser or emitter change was made. The existing precedence chain
(`or -> and -> cmp -> sum -> term -> factor`) already gives deterministic
grouping for expressions that avoid parentheses; the missing syntax is
recorded for a coordinated language round.
