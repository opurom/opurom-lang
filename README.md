# opurom-lang — `opuromc`

The language/compiler workstream of OPUROM. An independent Rust 1.99.1 crate (std +
`serde_json` only) with one binary, `opuromc`. The frontend lowers `.opu` source to
[IR v0.1](../opurom-spec/IR_SCHEMA.md); the backend lowers IR into target-native source.
**No target ships a runtime IR interpreter**: every emitter writes straight-line code from the
tree. R5 is additive: a program without R4/R5 features produces byte-identical R1-R3 IR, and
`counter.opu`, `profile.opu` and `todo.opu` keep byte-identical R4 IR. R6 is additive too:
custom components are expanded at compile time, so a program without them produces
byte-identical R5 IR and gains no new IR fields.

## Grammar summary (LANG_SPEC v0.1 + R2 + R4 + R5 + R6)

```ebnf
program       = { import_decl | component_decl } "app" IDENT "{" { state_decl | computed_decl | component_decl | model_decl | api_decl | theme_decl | node } "}" ;
component_decl = "component" IDENT "(" [ params ] ")" "{" { node } "}" ;
params        = param { "," param } [ "," ] ;
param         = IDENT [ ":" ( "int" | "bool" | "string" ) ] ;
import_decl   = "import" STRING [";"] ;
state_decl    = "state" IDENT "=" ( literal | string_list ) [";"] ;
computed_decl = "computed" IDENT "=" computed_expr [";"] ;
model_decl    = "model" IDENT "{" { field_decl } "}" ;
field_decl    = IDENT ":" ( "int" | "string" | "bool" ) [ "primary" ] [ "=" literal ] [";"] ;
api_decl      = "api" STRING "{" { handler_decl } "}" ;
handler_decl  = ( "list" | "get" | "create" | "update" | "delete" ) IDENT [";"] ;
theme_decl    = "theme" "{" ( "mode" | "accent" | "radius" ) ":" value { "," } "}" ;
node          = KIND [ "(" [ args ] ")" ] [ "async" ] [ ";" | "{" { node | statement | effect } "}" ]
              | "if" "(" IDENT ")" "{" { node } "}" [ "else" "{" { node } "}" ]
              | "for" "(" IDENT "in" ( INT | IDENT ) ")" "{" { node } "}" ;
args          = ( IDENT ":" value | value ) { "," ... } [ "," ] ;
value         = INT | FLOAT | STRING | BOOL | IDENT ;
statement     = IDENT ( "+=" | "-=" | "=" ) expr [";"]
              | IDENT "." "toggle" "(" ")" [";"] ;
effect        = "await" "delay" "(" INT ")" [";"]
              | "await" "fetch" "(" STRING "," IDENT ")" [";"] ;
expr          = literal | IDENT ;
string_list   = "[" [ STRING { "," STRING } [ "," ] ] "]" ;
computed_expr = computed_term { ( "+" | "-" ) computed_term } ;
computed_term = computed_factor { "*" computed_factor } ;
computed_factor = [ "-" ] INT | IDENT ;
```

- UTF-8, `.opu` extension, `//` line comments, insignificant whitespace (a newline is a valid
  node terminator, so `;` is optional).
- Kinds: the eight v0.1 kinds (`Column`, `Row`, `Container`, `Text`, `Button`, `Input`, `Image`,
  `Spacer`), the sixteen R2 kinds (`Card`, `Divider`, `Icon`, `Badge`, `Progress`, `Switch`,
  `Slider`, `Checkbox`, `Radio`, `Dialog`, `Tabs`, `List`, `Grid`, `Scroll`, `Avatar`, `Spinner`),
  the eight R4 primitives (`Table`, `Chart`, `Calendar`, `RichText`, `Player`, `Map`, `Menu`,
  `Tooltip`) and the 68 R10 primitives (listed in the `opurom-render` README). Catalog names are
  also accepted and lower to one of these 100 primitives.
  `Text("x")` and `Button("OK")` are the positional forms of `Text(value: "x")` /
  `Button(label: "OK")`.
- Types are inferred from the initializer: `int` (i64), `bool`, `string` and R5 `list`.
  `Text` values interpolate `{name}` against state; `{{` is a literal `{` (and `}}` a
  literal `}`). Computed names are readable in interpolation; list states are not.
- `Input(bind: name)` lowers to an `input` event whose `set` action omits `value`; the runtime
  supplies the string.
- `fn`, `module`, `units`, `dp`, `rem`, `let` and `var` remain rejected with a
  "not yet supported" diagnostic. `async`, `await` and `import` are R4 syntax with strict
  validation; `else`, `computed` and list-valued state are R5 syntax; `component` is R6 syntax
  with compile-time expansion only.

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

`for` integer counts are limited to 0..64; colors are `#RRGGBB`; `opacity` is 0..1; `shadow` is
0..8.

### R4 async/await effects

```opu
app AsyncDemo {
  state count = 0
  state payload = "idle"

  Button("Load") async {
    await delay(50)
    await fetch("/todos", payload)
    count += 1
  }
}
```

Only an `async` tap-handler block may contain `await`. `await delay(INT)` accepts 0..60000 ms;
`await fetch(STRING, IDENT)` performs a GET at runtime and stores the body in a declared string
state. Duplicate `async`, `await` outside an async block, an unknown effect, a bad call shape, a
non-string or undeclared `fetch` target and an out-of-range delay are diagnostics.

The event gains optional `"async": true` and an `"effects"` array. Effects are compile-time
lowered, not interpreted. Emitters run effects in source order before applying `actions`. An
R1-R3 consumer ignores `async` and `effects` and still applies `actions`; plain events gain
neither key.

### R4 modules

```opu
import "models/todo.opu"
import "ui/cards.opu"

app TodoApp { ... }
```

Imports appear before the app block. An imported `.opu` file may declare `state`, `model`,
`api` and `theme` only, either as bare declarations or inside an `app` wrapper; nested imports
are allowed. Declarations are merged before type checking. Duplicate names, duplicate themes,
duplicate API paths, cycles, `..` traversal, absolute paths, missing files, non-`.opu` paths and
disallowed declarations produce diagnostics. Paths resolve relative to the importing file, then
against `moduleRoots` from the nearest `opurom.json`. The IR gains an optional top-level
`"imports": ["models/todo.opu", ...]` array containing the direct imports, and consumers may
ignore it. R5 computed declarations are main-program only; an imported module with `computed`
gets a friendly diagnostic.

### R5 language features

```opu
app R5Demo {
  state count = 1
  state dark = false
  state name = "Ada"
  state items = ["a", "b"]
  computed doubled = count * 2
  computed label = name
  computed visible = dark

  Column {
    Text("Double: {doubled}")
    if (dark) {
      Text("dark")
    } else {
      Text("light")
    }
    for (item in items) {
      Text("{item}")
    }
    Button("go") { count += 1 }
  }
}
```

- `else` follows exactly one `if (boolState) { ... }` block. True-branch nodes keep `"when"`;
  else-branch nodes gain `"unless"`. Both fields are compile-time-only IR annotations. v0.1-R4
  consumers ignore `unless`; this round's web, mini, native and wasm emitters honor it.
  `else if`, `else` without `if`, duplicate `else`, an `else` on a non-bool state and a nested
  `else` inside an `else` branch are friendly diagnostics.
- `computed NAME = EXPR` declares a read-only derived `int`, `bool` or `string` value. The R5
  expression grammar is only `+`, `-`, `*`, integer literals and declared state names. Bool and
  string computed values come from a matching bool or string state; arithmetic requires int
  operands. Unknown names, computed-to-computed reads, list reads, string/bool operands,
  parentheses, floats, division, duplicate names and writes to computed values are diagnostics.
  IR gains an optional top-level `"computed"` array, and each computed name is also materialized
  in `"state"` with its compile-time initial value:

  ```json
  "computed": [{"name": "doubled", "type": "int", "expr": "count * 2"}]
  ```

  Rust targets use saturating i64 arithmetic; JS targets use their normal `Number` arithmetic.
  R4 consumers ignore the `"computed"` array and therefore see only the compiled initial value;
  this is the documented fallback, not a silent R5 recomputation.
- `state items = ["a", "b"]` declares a `"list"` state whose `init` is a JSON string array. A
  `for (item in items) { Text("{item}") }` block unrolls at compile time against that
  initializer; list values may not contain braces in R5, and a list loop is limited to 64
  values. Binding a list, interpolating a list, mutating a list, using it in a computed
  expression or looping over a non-list state is a friendly diagnostic. Runtime list mutation
  is R7+.

  ```json
  {"name": "items", "type": "list", "init": ["a", "b"]}
  ```

Plain programs gain no `computed` array, no `unless` fields and no `list` state entries. Byte-level
golden IR fixtures cover `counter.opu`, `profile.opu` and `todo.opu`.

### R6 custom components (compile-time expansion)

```opu
component Metric(label: string, value: int, active: bool) {
  Row {
    Text(label)
    Progress(value: value)
    Text("on: {active}")
  }
}

app R6Demo {
  Column {
    Metric("CPU", 42, true)
    Metric("RAM", 8, false)
  }
}
```

Declarations may appear before the `app` block or at the top level of its body. A component name
starts with an uppercase letter and a use site is `Name(arg, ...)`, with one literal per parameter.
`Name` also shadows a built-in primitive or catalog name of the same spelling: declaring
`component Card(title)` means `Card("Hi")` is the custom component, not the R2 `Card` primitive.
`int`, `bool` and `string` are the parameter types; omitting the annotation infers the type from
the first use site and checks later use sites against it.

Expansion happens in the compiler frontend: every use site is replaced by a copy of the component
body with parameter identifiers and `{param}` inside string props replaced by the literal
argument, and the same literals reach tap-handler statement values (`count += step` with
`step: int`). Bodies may contain ordinary nodes, `if`/`else`, `for`, nested component uses and
tap handlers. Copies are then lowered exactly like hand-written nodes, with pre-order ids, ordinary
`kind`/`props`/`children`/`events` and no `component` key. For the example above the first `Row`
becomes:

```json
{"id": 1, "kind": "Row", "props": {}, "children": [
  {"id": 2, "kind": "Text", "props": {"value": "CPU"}, "children": [], "events": {}},
  {"id": 3, "kind": "Progress", "props": {"value": 42}, "children": [], "events": {}},
  {"id": 4, "kind": "Text", "props": {"value": "on: true"}, "children": [], "events": {}}
], "events": {}}
```

There is **no runtime component registry and no IR interpreter**: the IR contains no component
declarations, no component nodes and no new top-level field, so all nine emitters, the optimizer
and the wasm ABI consume it unchanged. `opuromc components` still lists only the 216 built-in
catalog names.

Friendly diagnostics cover recursive and mutually cyclic components (including unused ones),
arity mismatch, unknown components (in the app and in bodies), parameter type mismatch, duplicate
component names, duplicate parameter names, unknown parameter types, named use-site arguments,
non-literal arguments, a parameter used where the language needs state (`if` conditions, `for`
iterables, statement targets), component declarations inside node blocks or imported modules, an
app name that collides with a component, and a component used as the top-level app instead of an
`app Name { ... }` block. Every diagnostic has a negative fixture and a test.

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

  state remaining = 0
  Column(p: 16) {
    Text("Remaining: {remaining}")
    Button("Refresh") { remaining += 1 }
  }
}
```

`model`/`api` lower to the additive top-level IR arrays `models` and `api` (see
[FULLSTACK_SPEC](../opurom-spec/FULLSTACK_SPEC.md) §2). The arrays appear only when the source
declares them. `ir` stays `"0.1"`.

Diagnostics go to stderr as `path:line:col: error: message` plus a `  help: ...` line; exit code
1 on any error, 0 on success. `check` is diagnostics-only (no stdout, no files).

## CLI

```sh
opuromc check <src.opu>                                   # diagnostics only
opuromc compile <src.opu> -o <out.opuir.json>             # lex -> parse -> typecheck -> IR
opuromc target <ir.json> --target <name> -o <dir>         # lower IR into target source
opuromc targets                                           # list all 9 targets
opuromc components                                        # list the 216 catalog names
opuromc optimize <ir.json> -o <out.json>                  # dead-state elimination
opuromc --version                                         # 0.2.0
```

The compiler emits node ids pre-order from 0, keeps numeric props as JSON numbers, puts
`events: {}` and `children: []` on every node, and adds the default theme
`{"mode":"system","accent":"#4F46E5","radius":12}`. Compiling
[`counter.opu`](../opurom-spec/counter.opu) is deep-equal to
[`counter.opuir.json`](../opurom-spec/counter.opuir.json); `counter.opu`, `profile.opu` and
`todo.opu` also have byte-level golden IR fixtures in `tests/fixtures/golden/`.

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
(extensions, loop/key directives, tap/input directives, state-set API, request API, window title
key and binding delimiters). All emitters generate code, not an IR parser. They understand the
R2/R4 IR plus R5 `when`, `unless`, `computed` and `list` state entries.

### R4 per-target effect status

| Target | Status | Honest implementation and limits |
| --- | --- | --- |
| `web` | **Full** | `async function applyTap`, `await` on a `Promise`/`setTimeout` delay, `await fetch(path)`, then `response.text()` into the string state. `node --check` is verified; no browser automation runs here. |
| `native` | **Full within the std-only contract** | A deterministic source-order sequencer uses `Instant`-measured `thread::sleep` (busy-free) and a `TcpStream` HTTP/1.1 GET against `OPR_BASE_URL`. With the variable unset, the documented local result is `local: <path>`. Plain HTTP only: no TLS, redirects, IPv6 literals or HTTP/2. |
| `wechat`, `alipay`, `qq`, `douyin`, `baidu`, `kuaishou` | **Approximate** | `async` handlers await `setTimeout` promises and call each platform's `request` API (`wx.request`, `my.request`, `qq.request`, `tt.request`, `swan.request`, `ks.request`). The generated URL is the literal IR path; a real deployment needs a configured absolute base and domain allowlist. No simulator or device conformance runs. |
| `wasm` | **Approximate** | `host.js` exposes `HOST_CALLBACKS` for `setTimeout` and `fetch`, runs them through `runEffects(id)` before the synchronous `opu_tap`, and keeps string state in the host mirror. The wasm module has no imports and no OS async IO; browser host behavior is not automated here. |

### R5 per-target computed status

| Target | Status | Honest implementation and limits |
| --- | --- | --- |
| `web` | **Full** | `applyComputed()` runs after every tap/input handler and before startup render; computed fields live in `state`; `unless` hides else subtrees. `node --check` is verified. |
| `native` | **Full** | `apply_computed()` runs after each event, returns computed patches, uses saturating i64 arithmetic and stores list states as `Vec<String>`. Generated crates build warning-free and run with piped stdin. |
| `wechat`, `alipay`, `qq`, `douyin`, `baidu`, `kuaishou` | **Full at generated-source level** | An `applyComputed` method calls `setData` after every event; `unless` renders as a negated conditional. JS `Number` arithmetic is used; no simulator or device conformance runs. |
| `wasm` | **Full with split state** | Int computed values are recomputed inside the wasm module and visible through `opu_state`; bool/string computed values are recomputed in the `host.js` mirror. Node ABI covers the int path; no browser automation or OS async IO runs. |

No R5 emitter silently ignores `computed`. A v0.1-R4 consumer may ignore `computed`/`unless` and
keeps only compiled initial values; that is the documented consumer-side fallback.

### R3/R4 component catalog

[components.json](../opurom-spec/components.json) is vendored **byte-for-byte** as
[src/catalog.json](src/catalog.json) and embedded with `include_str!`, so the crate builds
standalone. Its 216 tuples are `[name, primitive, category, defaultProps]`, mapping onto
100 distinct primitive kinds (32 R1-R9 + 68 R10). A source `Kind` that is a
catalog name lowers to the named primitive: the IR
node keeps `kind` = primitive and gains `"component": "<name>"`. Consumers that ignore
`component` still read valid v0.1 IR.

`defaultProps` are injected after explicitly written props are validated, so explicit values
win and preset keys need not be props that the primitive declares. The catalog is validated at
compile time. `opuromc components` prints `name | primitive | category` rows and a final
`216 components` line. Unknown kinds remain a diagnostic. The R3 accessibility keys `label`,
`role`, `fontScale` and `highContrast` are accepted on any node and preserved in IR props.

### R10 component catalog expansion (R10_ADDENDUM section 1)

The vendored [components.json](../opurom-spec/components.json) now has **216
named components mapped onto 100 DISTINCT primitive kinds**: the 32 R1-R9 kinds
plus 68 new ones. Each new primitive has its own deterministic software drawing,
a documented prop schema and a default a11y role in `opurom-render`; the catalog
itself is still plain data (name, primitive, category, defaultProps). 100 new
names were added: 68 canonical names (one per new kind) and 32 secondary names
that expand to a new primitive at compile time. `opuromc components` prints the
new `216 components` count, and the compiler's `KINDS` validator accepts all 100
primitives through one documented, reasonable prop schema (all props optional,
`value` positional, no children, no tap handler).

`opuromc check` / `compile` accept every new kind and catalog name, and all nine
emitters still lower an IR containing them. Plain `counter.opu`, `profile.opu`
and `todo.opu` keep byte-identical IR: the new kinds are additive and no existing
kind changed its lowering.

**Honest scope:** one deterministic software drawing per kind (in
`opurom-render`), no real font shaping, no image decoding, no platform-native
widgets and no frame-rate / 120fps claim. Catalog names remain compile-time
aliases for primitives; they are not runtime components.

### R3/R5 optimizer

`opuromc optimize <ir.json> -o <out.json>` performs dead-state elimination: it removes `state`
entries that no interpolation, `bind`, event action `target` or `valueFrom`, `when`, `unless`,
R4 fetch `into` or R5 computed expression can observe. It drops props whose value is `null`,
keeps empty `children`/`events` objects, preserves the optional top-level `computed` array and
preserves `list` states and computed state entries. It never removes nodes, folds conditionals,
changes ids, reorders state, strips `component`, or mutates `imports`/`effects`. IR versions other
than `0.1` and fields outside the additive IR schema are rejected with a diagnostic and exit
code 1.

Measured byte counts are printed to stdout as `optimize: <in> bytes in, <out> bytes out`; both
numbers come from the actual files. A golden IR with no dead state optimizes to a deep-equal IR.
Optimization is opt-in: default `compile` output stays byte-identical.

## Verify

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cd /Users/mac/work/opurom/opurom-lang
cargo fmt --all
cargo fmt --all -- --check
cargo test --offline                 # 55 tests: 4 unit + 45 CLI + 6 wasm
RUSTFLAGS="-D warnings" cargo test --offline   # zero compiler warnings
```

The test harness drives the CLI binary from `tests/fixtures/`, compares golden IR byte-for-byte
and parsed-JSON deep-equality, checks `node --check` on web + six mini `index.js`/`app.js` files
plus the wasm `host.js`, runs `cargo build --release --offline` inside generated native crates
followed by piped-stdin sessions, and builds generated wasm crates for
`wasm32-unknown-unknown` before instantiating them in Node's `WebAssembly` runtime with an empty
import object. R5 tests cover `else`/`unless`, computed values, list unroll, every new diagnostic,
all nine emitters, computed native runtime patches and computed wasm ABI state. R6 tests cover
one, two, nested and in-app component uses, the exact expanded IR, all fifteen negative classes,
all nine emitters and the optimizer on expanded IR, byte-identical plain IR, and the wasm ABI on
a component-expanded tree.

## Honest status

**Verified in this round**

- `cargo test --offline`: 55/55 tests pass (4 unit + 45 CLI + 6 wasm); `RUSTFLAGS="-D warnings"`
  is clean and `cargo fmt --all -- --check` passes.
- UI-only programs (`counter.opu`, `profile.opu`, `todo.opu`) compile to **byte-identical** IR
  compared with R4; no `imports`, `async`, `effects`, `computed`, `unless` or component keys are
  added to plain IR. The 216/100 vendored catalog is byte-identical to the spec catalog.
- Negative fixtures cover the v0.1 classes, R2 classes, R4 async/import classes, R5
  `else`/computed/list classes and fifteen R6 component classes. No panics, no stack traces.
- R6 components expand in the frontend: one, two, nested and in-app uses produce ordinary IR
  with pre-order ids, and all 9 emitters, the optimizer and the wasm ABI accept that IR.
- The wasm target builds with `cargo build --release --target wasm32-unknown-unknown` and the
  `.wasm` instantiates in Node with an empty import object; `opu_version() == 1`, int computed
  state is recomputed after a tap, and R4 effects remain host-side callbacks without wasm
  imports.
- Every generated JS file passes `node --check` (web + 6 mini + wasm host); `opuromc targets`
  lists all 9 targets and `opuromc components` prints `216 components`.
- Generated native output builds warning-free with `cargo build --release --offline` and runs with
  piped stdin. The R4 native sequencer's local fallback and plain-HTTP `OPR_BASE_URL` path are
  generated from the same IR.

### Scope and limits

- **216 named components map onto 100 distinct primitive kinds.** The 68 R10 kinds each
  have a distinct deterministic software drawing in `opurom-render`; the other 32 are the
  R1-R9 primitives. Catalog names are compile-time aliases, not renderer components. R6
  custom components are compile-time macros, not renderer components: no runtime registry
  and no component node in the IR. Honest scope: no real font shaping, no image decoding,
  no native widgets and no fps claim.
- **R6 components take literal arguments only.** Parameters are `int`, `bool` or `string`;
  substituting into `text` props and literal attribute values is the whole feature. Passing a
  state name, an expression, a list or a node is rejected. Components are not generic and have
  no conditional/loop parameters: conditional UI still comes from `if`/`else` and `for`.
  Component expansion is not size-capped beyond the existing 64-iteration `for` limit.
- **The optimizer is dead-state elimination only.** It does not prune nodes, fold conditionals,
  remove events or reorder anything; R5 merely adds `computed`, `unless` and `list` preservation.
- **Effects run before actions.** The IR has separate `effects` and `actions` arrays; source-level
  interleaving after an `await` is intentionally not represented. Wasm has no OS async IO.
- **Modules are compile-time declaration merges.** There is no runtime module registry, no remote
  resolver, no code execution during import resolution and no computed or component declarations
  in imported modules.
- **R5 lists are compile-time constants.** A `for` loop unrolls against the initializer, list
  values may not contain braces, list loops are capped at 64 values and runtime list mutation is
  not emitted.

**Still R7+**

- OS async IO in wasm, runtime list mutation, generic or conditional components, component
  libraries shared through imports, nested `else if` chains, Vulkan/ANGLE/D3D backends, full GPU
  UI rasterization, Windows/Linux/Harmony native windows, real device conformance and fps claims,
  TLS, SCRAM, MySQL, remote marketplace, an IntelliJ runtime plugin, OS screen-reader integration
  and competitive benchmark claims.
- LSP, plugin sandboxing and a perf harness remain later work. No browser/DOM automation, no
  simulator or device conformance, no pixel golden hash and no invented numbers are claimed here.
