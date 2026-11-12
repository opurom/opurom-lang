# opurom-lang — `opuromc`

The language/compiler workstream of OPUROM. An independent Rust 1.99.1 crate (std +
`serde_json` only) with one binary, `opuromc`. The frontend lowers `.opu` source to
[IR v0.1](../opurom-spec/IR_SCHEMA.md); the backend lowers IR into target-native source.
**No target ships a runtime IR interpreter**: every emitter writes straight-line code from the
tree.

## Grammar summary (LANG_SPEC v0.1)

```ebnf
program    = "app" IDENT "{" { state_decl | node } "}" ;
state_decl = "state" IDENT "=" literal [";"] ;
node       = KIND [ "(" [ args ] ")" ] [ ";" | "{" { node | statement } "}" ] ;
args       = ( IDENT ":" value | value ) { "," ... } [ "," ] ;
value      = INT | STRING | BOOL | IDENT ;
statement  = IDENT ( "+=" | "-=" | "=" ) expr [";"]
           | IDENT "." "toggle" "(" ")" [";"] ;
expr       = literal | IDENT ;
```

- UTF-8, `.opu` extension, `//` line comments, insignificant whitespace (a newline is a valid
  node terminator, so `;` is optional).
- Kinds: `Column`, `Row`, `Container` (children), `Text` (`value`), `Button` (`label` + one tap
  statement), `Input` (`placeholder`, `bind`), `Image` (`src`), `Spacer`. `Text("x")` and
  `Button("OK")` are the positional forms of `Text(value: "x")` / `Button(label: "OK")`.
- Types are inferred from the initializer: `int` (i64), `bool`, `string`. `Text` values
  interpolate `{name}` against state; `{{` is a literal `{` (and `}}` a literal `}`).
- `Input(bind: name)` lowers to an `input` event whose `set` action omits `value`; the runtime
  supplies the string.
- `async`, `await`, `if`, `for`, `fn`, `import` are rejected with a
  "not yet supported in v0.1" diagnostic.

Diagnostics go to stderr as `path:line:col: error: message` plus a `  help: ...` line; exit code
1 on any error, 0 on success. `check` is diagnostics-only (no stdout, no files).

## CLI

```sh
opuromc check <src.opu>                                   # diagnostics only
opuromc compile <src.opu> -o <out.opuir.json>             # lex -> parse -> typecheck -> IR
opuromc target <ir.json> --target <name> -o <dir>         # lower IR into target source
opuromc targets                                           # list all 8 targets
```

The compiler emits node ids pre-order from 0, keeps numeric props as JSON numbers, puts
`events: {}` and `children: []` on every node, and adds the default theme
`{"mode":"system","accent":"#4F46E5","radius":12}`. Compiling
[`counter.opu`](../opurom-spec/counter.opu) is deep-equal to
[`counter.opuir.json`](../opurom-spec/counter.opuir.json).

## Emitters

| Target | Artifacts | Lowering notes |
| --- | --- | --- |
| `native` | `Cargo.toml`, `src/main.rs` | std-only `State` struct, `apply(event_node_id)` with a generated `match` over tap/input actions, stdin loop `tap <id>` / `input <id> <text>` / `state` / `quit` printing state and patches |
| `web` | `index.html`, `app.js`, `ir.json` | generated `state` object, event handlers, Canvas 2D draw calls with computed layout, `{}` interpolation resolved to `String(state.x)` (no mustache, no IR traversal) |
| `wechat` | `app.json`, `index.wxml`, `index.wxss`, `index.js` | `wx:for`, `bindtap`/`bindinput`, `setData` |
| `alipay` | `app.json`, `index.axml`, `index.acss`, `index.js` | `a:for`, `onTap`/`onInput`, `setData` |
| `qq` | `app.json`, `index.qml`, `index.qss`, `index.js` | `qq:for`, `bindtap`/`bindinput`, `setData` |
| `douyin` | `app.json`, `index.ttml`, `index.ttss`, `index.js` | `tt:for`, `bindtap`/`bindinput`, `setData` |
| `baidu` | `app.json`, `index.swan`, `index.css`, `index.js` | `s-for`, `bindtap`/`bindinput`, `setData` |
| `kuaishou` | `app.json`, `index.ksml`, `index.css`, `index.js` | `ks:for`, `bindtap`/`bindinput`, `setData` |

The six mini-program targets share one lowering pass and one small per-platform table
(extensions, loop/key directives, tap/input directives, state-set API, window title key and
binding delimiters). Web and mini emitters generate code, not an IR parser.

## Verify

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cd /Users/mac/work/opurom/opurom-lang
cargo test --offline                 # 10 integration tests
RUSTFLAGS="-D warnings" cargo test --offline   # zero compiler warnings
```

The test harness drives the CLI binary from `tests/fixtures/` (21 negative fixtures covering
every error class), compares the golden IR with deep equality, checks `node --check` on web +
six mini `index.js`/`app.js` files, and runs `cargo build --release --offline` inside the
generated native crate followed by a piped-stdin session.

## Honest status

**Verified in this round**

- `cargo test --offline`: 10/10 tests pass, including the golden `counter.opu` →
  `counter.opuir.json` parsed-JSON deep-equality assertion.
- 21 negative fixtures: duplicate state, unknown state in events/interpolation/`bind`,
  add/toggle type misuse, assignment mismatch, unknown node kind/prop, missing required prop,
  syntax errors, mixed blocks and all five v0.2 keywords. No panics, no stack traces.
- Every generated JS file passes `node --check` (web + 6 mini targets); `opuromc targets`
  lists all 8 targets.
- Generated native output builds warning-free with `cargo build --release` and runs with piped
  stdin (`state ...` and `patch ...` lines verified).
- `RUSTFLAGS="-D warnings" cargo test --offline` is clean.

**Planned / not verified (later rounds)**

- WASM emitter + wasm32 build, GPU/ANGLE/Metal/Vulkan backend, LSP + debugger, animation and
  theme DSL, custom components, units/tokens, plugin sandboxing, perf harness — R2+.
- `async`/`await`, `if`/`for` in UI, `fn`, modules/imports: rejected by design in v0.1.
- Event expressions that read another state (`a = b`, `a += b`) are rejected with a friendly
  v0.1 diagnostic: IR v0.1 has no dynamic-expression action. Literal `set`/`add` are supported.
- A Button tap block holds exactly one statement; container blocks cannot hold statements.
  Multiple top-level nodes are wrapped in a synthesized `Column` root (id 0).
- Web target: Canvas 2D draws every node, and `Input` is also exposed as a DOM `<input>` for
  keyboard events. Only `node --check` is verified; no browser/DOM automation runs here.
- Mini-program markup is verified at the syntax/artifact level only (files, directives,
  `node --check` on generated JS). No simulator or device conformance runs, no pixel golden
  hash (that contract belongs to `opurom-render`).
- Native target is a std-only command loop with state + patches; it has no renderer or widgets.
- Theme is always the compiler default; source theme syntax is v0.2.
