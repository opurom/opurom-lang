# opurom-lang — `opuromc`

The language/compiler workstream of OPUROM. An independent Rust 1.99.1 crate (std +
`serde_json` only) with one binary, `opuromc`. The frontend lowers `.opu` source to
[IR v0.1](../opurom-spec/IR_SCHEMA.md); the backend lowers IR into target-native source.
**No target ships a runtime IR interpreter**: every emitter writes straight-line code from the
tree.

## Grammar summary (LANG_SPEC v0.1 + LANG_R2_ADDENDUM)

```ebnf
program    = "app" IDENT "{" { state_decl | model_decl | api_decl | theme_decl | node } "}" ;
state_decl = "state" IDENT "=" literal [";"] ;
model_decl = "model" IDENT "{" { field_decl } "}" ;
field_decl = IDENT ":" ( "int" | "string" | "bool" ) [ "primary" ] [ "=" literal ] [";"] ;
api_decl   = "api" STRING "{" { handler_decl } "}" ;
handler_decl = ( "list" | "get" | "create" | "update" | "delete" ) IDENT [";"] ;
theme_decl = "theme" "{" ( "mode" | "accent" | "radius" ) ":" value { "," } "}" ;
node       = KIND [ "(" [ args ] ")" ] [ ";" | "{" { node | statement } "}" ]
           | "if" "(" IDENT ")" "{" { node } "}"
           | "for" "(" IDENT "in" INT ")" "{" { node } "}" ;
args       = ( IDENT ":" value | value ) { "," ... } [ "," ] ;
value      = INT | FLOAT | STRING | BOOL | IDENT ;
statement  = IDENT ( "+=" | "-=" | "=" ) expr [";"]
           | IDENT "." "toggle" "(" ")" [";"] ;
expr       = literal | IDENT ;
```

- UTF-8, `.opu` extension, `//` line comments, insignificant whitespace (a newline is a valid
  node terminator, so `;` is optional).
- Kinds: the eight v0.1 kinds (`Column`, `Row`, `Container`, `Text`, `Button`, `Input`, `Image`,
  `Spacer`) plus the sixteen R2 kinds (`Card`, `Divider`, `Icon`, `Badge`, `Progress`, `Switch`,
  `Slider`, `Checkbox`, `Radio`, `Dialog`, `Tabs`, `List`, `Grid`, `Scroll`, `Avatar`, `Spinner`).
  `Text("x")` and `Button("OK")` are the positional forms of `Text(value: "x")` /
  `Button(label: "OK")`.
- Types are inferred from the initializer: `int` (i64), `bool`, `string`. `Text` values
  interpolate `{name}` against state; `{{` is a literal `{` (and `}}` a literal `}`).
- `Input(bind: name)` lowers to an `input` event whose `set` action omits `value`; the runtime
  supplies the string.
- `async`, `await`, `fn`, `import`, `else` are rejected with a "not yet supported" diagnostic.

### R2 language features

| Feature | Source | IR |
| --- | --- | --- |
| Multi-statement handler | `Button("+2") { count += 1; count += 1 }` | `{"actions":[...]}` |
| State-to-state action | `Button("copy") { a = b }` | `{"op":"set","target":"a","valueFrom":"b"}` |
| Conditional UI | `if (dark) { Text("...") }` | `"when": "dark"` on each direct child |
| Compile-time repetition | `for (i in 3) { Text("Item {i}") }` | unrolled nodes, consecutive ids |
| Theme block | `theme { mode: dark, accent: "#FF4D6D", radius: 8 }` | `IR.theme` |
| Atomic shortcuts | `p` / `m` / `bg` / `rounded` | `padding` / `margin` / `color` / `radius` |
| Visual props | `shadow`, `gradient`, `glass`, `opacity`, `border`, `animate` | passed through |

`for` counts are limited to 0..64; colors are `#RRGGBB`; `opacity` is 0..1; `shadow` is 0..8.

### Full-stack syntax (FULLSTACK_SPEC)

```opu
app TodoApp {
  model Todo {
    id: int primary
    title: string
    done: bool = false
  }

  api "/todos" {
    list Todo
    create Todo
  }

  api "/todos/:id" {
    get Todo
    update Todo
    delete Todo
  }

  state remaining = 0
  Column(p: 16) {
    Text("Remaining: {remaining}")
    Button("Refresh") { remaining += 1 }
  }
}
```

`model`/`api` lower to the additive top-level IR arrays `models` and `api` (see
[FULLSTACK_SPEC](../opurom-spec/FULLSTACK_SPEC.md) §2). The arrays appear **only** when the
source declares them: a UI-only program keeps byte-identical v0.1 IR. `ir` stays `"0.1"`.

Diagnostics go to stderr as `path:line:col: error: message` plus a `  help: ...` line; exit code
1 on any error, 0 on success. `check` is diagnostics-only (no stdout, no files).

## CLI

```sh
opuromc check <src.opu>                                   # diagnostics only
opuromc compile <src.opu> -o <out.opuir.json>             # lex -> parse -> typecheck -> IR
opuromc target <ir.json> --target <name> -o <dir>         # lower IR into target source
opuromc targets                                           # list all 9 targets
opuromc components                                        # list the 108 catalog names
opuromc optimize <ir.json> -o <out.json>                  # dead-state elimination
opuromc --version                                         # 0.2.0
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
| `wasm` | `Cargo.toml`, `src/lib.rs`, `index.html`, `host.js`, `README.md` | std-only `cdylib` for `wasm32-unknown-unknown` with the C ABI of [WASM_ABI](../opurom-spec/WASM_ABI.md): `opu_version`, `opu_state_len`, `opu_state`, `opu_tap`, `opu_input` |

The six mini-program targets share one lowering pass and one small per-platform table
(extensions, loop/key directives, tap/input directives, state-set API, window title key and
binding delimiters). Web and mini emitters generate code, not an IR parser.

All emitters understand the R2 IR: an event is one action object or an `actions` array, an
action may read another state through `valueFrom`, a node may carry `when`, and a node kind a
consumer does not know renders as a `Container` (with children) or a `Text`
(label/value/title/name) — the documented fallback, not an error.

### R3 component catalog

[components.json](../opurom-spec/components.json) is vendored as
[src/catalog.json](src/catalog.json) and embedded with `include_str!`, so the crate builds
standalone. Its 108 tuples are `[name, primitive, category, defaultProps]`. A source `Kind` that
is a catalog name lowers to the named primitive: the IR node keeps `kind` = primitive and gains
`"component": "<name>"`. Consumers that ignore `component` still read valid v0.1 IR.

`defaultProps` are injected **after** explicitly written props are validated, so explicit values
win and preset keys need not be props that the primitive declares. The catalog is validated at
compile time: duplicate names, unknown primitives and malformed (non-flat/non-scalar) defaults
are rejected. Expansion is deterministic and compile-time only; no component registry ships in a
build. `opuromc components` prints `name | primitive | category` rows and a final
`108 components` line. Unknown kinds remain a diagnostic.

The R3 accessibility keys `label`, `role`, `fontScale` and `highContrast` are accepted on any
node and preserved in IR props. This crate only produces them; rendering them belongs to
`opurom-render`.

### R3 optimizer

`opuromc optimize <ir.json> -o <out.json>` performs dead-state elimination: it removes `state`
entries that no interpolation (`{name}` in any string prop), `bind`, event action `target` or
`valueFrom`, or `when` can observe. It drops props whose value is `null` and keeps empty
`children`/`events` objects (the schema requires them). It never removes nodes, folds `when`,
changes ids, reorders state, or strips the optional `component` field. IR versions other than
`0.1` and fields outside the IR schema are rejected with a diagnostic and exit code 1.

Measured byte counts are printed to stdout as `optimize: <in> bytes in, <out> bytes out` — both
numbers come from the actual input and output files, not estimates. A golden IR with no dead state
optimizes to a deep-equal IR. Optimization is opt-in: default `compile` output stays byte-identical
to R2.

## Verify

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cd /Users/mac/work/opurom/opurom-lang
cargo test --offline                 # 26 integration + 4 unit tests
RUSTFLAGS="-D warnings" cargo test --offline   # zero compiler warnings
```

The test harness drives the CLI binary from `tests/fixtures/`, compares the golden IR with deep
equality, checks `node --check` on web + six mini `index.js`/`app.js` files plus the wasm
`host.js`, runs `cargo build --release --offline` inside the generated native crate followed by
a piped-stdin session, and builds the generated wasm crate for `wasm32-unknown-unknown` before
instantiating it in Node's `WebAssembly` runtime with an empty import object. R3 tests cover
catalog/spec parity, catalog expansion with explicit-props-win, the `component` field, all 24
primitive kinds, the `components` count, and optimizer no-op/shrink/id-order/bad-IR cases.

## Honest status

**Verified in this round**

- `cargo test --offline`: 30/30 tests pass (26 integration + 4 unit), including the golden
  `counter.opu` → `counter.opuir.json` parsed-JSON deep-equality assertion.
- UI-only programs (`counter.opu`, `profile.opu`) compile to **byte-identical** IR compared
  with the R1 compiler; no empty `models`/`api` arrays are added.
- 36 negative fixtures: the 21 v0.1 classes plus the R2 classes (model type, two primaries,
  default mismatch, duplicate model, unknown api model, unknown handler verb, missing path
  parameter, `else`, `for` count > 64, bad color, opacity/shadow range, `valueFrom` type
  mismatch, non-bool `when`, duplicate theme, unknown theme key, unknown animate value). No
  panics, no stack traces.
- The wasm target builds with `cargo build --release --target wasm32-unknown-unknown` and the
  `.wasm` instantiates in Node with an empty import object; `opu_version() == 1`,
  `opu_state(0) == init`, `opu_tap(plus) == 1`, `opu_state(0) == init + 1`.
- Every generated JS file passes `node --check` (web + 6 mini + wasm host); `opuromc targets`
  lists all 9 targets.
- Generated native output builds warning-free with `cargo build --release` and runs with piped
  stdin (`state ...` and `patch ...` lines verified).
- `RUSTFLAGS="-D warnings" cargo test --offline` is clean.

### R3 status

- **108 named components map onto the 24 primitives.** There are no distinct renderer
  implementations per component: a component is a compile-time name plus preset props, resolved
  to a primitive before any emitter sees the tree.
- **The optimizer is dead-state elimination only, not full tree shaking.** It does not prune
  nodes, fold conditionals, remove events or reorder anything; it only removes unreachable state
  entries and null-valued props. No byte counts are estimated — both numbers are measured from
  the input and output files.
- Verified: catalog/spec parity, compile-time catalog validation (duplicates, unknown
  primitives, malformed props), explicit-props-win expansion, `component` field presence only
  on catalog nodes, `opuromc components` count >= 100, all 24 primitive kinds unchanged,
  golden IR optimizing to a deep-equal IR, dead-state shrinking, id/order/`when`/`component`
  preservation, and bad-IR diagnostics.

**Planned / not verified (later rounds)**

- GPU/ANGLE/Metal/Vulkan backend, LSP, user-defined custom components, plugin sandboxing,
  perf harness — R4+.
- `async`/`await`, `fn`, modules/imports, `else`: rejected by design.
- Event handlers with local variables and list-valued state are out of scope for R2.
- The wasm host renders with Canvas 2D and forwards taps; there is no DOM/WASM hybrid
  rendering, no SIMD, no threads and no startup benchmark (R3+ per WASM_ABI.md).
- Bool and string states live in the wasm host as a mirror (`host.js`); only int states cross
  the ABI in R2.
- Web target: Canvas 2D draws every node, and `Input` is also exposed as a DOM `<input>` for
  keyboard events. Only `node --check` is verified; no browser/DOM automation runs here.
- Mini-program markup is verified at the syntax/artifact level only (files, directives,
  `node --check` on generated JS). No simulator or device conformance runs, no pixel golden
  hash (that contract belongs to `opurom-render`).
- Native target is a std-only command loop with state + patches; it has no renderer or widgets.
- The `models`/`api` IR arrays are consumed by `opurom-server` and `opurom-db`; this crate only
  produces them.
