# Development

How to work on this repository: layout, prerequisites, build and test
commands, how a plug-in repository consumes the framework, the hot-reload
workflow, conventions, documentation, CI, and the platform quirks that cost
time.

## Layout

```
Cargo.toml                 workspace: noob-vst-webgui-framework, noob-vst-webgui-framework-webview, noob-vst-webgui-framework-nih; the nih-plug [patch]
package.json               @noob-audio-engineering/noob-vst-webgui-framework, installable from git; re-exports crates/noob-vst-webgui-framework/web
crates/
  noob-vst-webgui-framework/          bridge, params, streams, wire codec, server, discovery, store (Rust)
  noob-vst-webgui-framework/web/      the browser package: noob-vst-webgui-framework.js, components/, vue/, examples/vanilla
  noob-vst-webgui-framework-webview/  wry wrapper: embed the OS web view, native UI timer (Rust)
  noob-vst-webgui-framework-nih/      nih-plug Editor adapter, StoreSlot (Rust)
docs/                      guides and references (this folder)
tools/                     node scripts: bench, setparam, play, instances
.github/workflows/         ci.yml (fmt, clippy, test, doc, adapter checks), docs.yml (publish rustdoc)
```

The plug-ins built on the framework live in their own repositories under
[Noob Audio Engineering](https://github.com/Noob-Audio-Engineering):
[noob-q](https://github.com/Noob-Audio-Engineering/noob-q),
[noob-wave](https://github.com/Noob-Audio-Engineering/noob-wave) and
[noob-compressorlab](https://github.com/Noob-Audio-Engineering/noob-compressorlab).
Each has the same shape: `src/dsp`, `src/plugin.rs`, `src/bin/standalone.rs`
and a Vite SPA under `web/`.

The rule that shapes the tree: **generic code goes in the framework,
product code goes in the plug-in.** If a component, composable, or Rust
helper could serve a different plug-in, it belongs in
`crates/noob-vst-webgui-framework/web/` or the `noob-vst-webgui-framework-*`
crates, and it stays headless and uncoloured: the framework imposes no look,
face or colour on a page. Everything visual stays in the plug-in that draws
it.

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | stable, edition 2024 (1.85+; developed on 1.96) | `rustup component add clippy rustfmt` |
| Node | 20+ | 22 recommended; the tools use the global `WebSocket` |
| Windows | 10/11 | WebView2 runtime is part of Windows 11 and most Windows 10 installs |
| macOS | 11+ | WKWebView is part of the OS |
| Linux | any recent | `libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libasound2-dev` for wry and cpal, plus the X11 dev packages nih-plug needs |

## Build

```sh
# the core crate (the default member)
cargo build

# every crate, including the web view wrapper and the nih-plug adapter
cargo build --workspace

# the adapter alone, which also compiles the patched nih-plug (see below)
cargo check -p noob-vst-webgui-framework-nih
cargo check -p noob-vst-webgui-framework-nih --release           # wry gates devtools on debug_assertions; release must build too
cargo check -p noob-vst-webgui-framework-nih --features devtools
```

The browser package has no build step: it is plain ES modules under
`crates/noob-vst-webgui-framework/web/`, syntax-checked with `node --check`
in CI.

## Patched nih-plug

Upstream nih-plug has no host-to-plugin resizing: its VST3 view answers
`canResize` with "no" and its CLAP gui extension rejects `set_size`, so a
window could only grow when the plug-in asked. The workspace therefore
builds against my fork, branch `host-resize` of
`https://github.com/Noob-Audio-Engineering/nih-plug`, through a `[patch]`
entry at the bottom of the root `Cargo.toml`. The fork adds three `Editor`
methods with backwards-compatible defaults, `can_resize`,
`check_size_constraint` and `set_size`, and implements them in the VST3 and
CLAP wrappers; the adapter in `crates/noob-vst-webgui-framework-nih`
implements them for the web view. Everything else is upstream as of commit
`f36931f7`.

Every plug-in repository needs the same `[patch]` in its own root manifest
until the change lands upstream, because Cargo applies patches only from
the root manifest. To move to a newer upstream, rebase the branch on it,
push, and run `cargo update -p nih_plug` here and in each plug-in.

## Using the framework from another repository

A plug-in repository takes the crates from git and the browser package from
this same repository; nothing is on crates.io or npm.

```toml
# Cargo.toml of the plug-in (one package, no workspace)
[dependencies]
noob-vst-webgui-framework = { git = "https://github.com/Noob-Audio-Engineering/noob-vst-webgui-framework" }
noob-vst-webgui-framework-nih = { git = "https://github.com/Noob-Audio-Engineering/noob-vst-webgui-framework", optional = true }
nih_plug = { git = "https://github.com/robbert-vdh/nih-plug.git", optional = true }

[features]
plugin = ["dep:nih_plug", "dep:noob-vst-webgui-framework-nih", "dep:include_dir"]

# Cargo applies patches only from the root manifest, so each plug-in repeats this
[patch."https://github.com/robbert-vdh/nih-plug.git"]
nih_plug = { git = "https://github.com/Noob-Audio-Engineering/nih-plug.git", branch = "host-resize" }
```

Add `rev = "..."` to pin a commit; `Cargo.lock` records the commit either
way, and `cargo update -p noob-vst-webgui-framework` moves to the newest.

```jsonc
// web/package.json of the plug-in
{
  "dependencies": {
    "@noob-audio-engineering/noob-vst-webgui-framework": "github:Noob-Audio-Engineering/noob-vst-webgui-framework",
    "vue": "^3.5"
  }
}
```

npm installs the repository and resolves the package through the root
`package.json`, whose `exports` point into
`crates/noob-vst-webgui-framework/web`. Two Vite settings make the Vue layer
use the app's copy of `vue` and keep the package out of the dependency
pre-bundle:

```js
// web/vite.config.js
resolve: { dedupe: ['vue'] },
optimizeDeps: { exclude: ['@noob-audio-engineering/noob-vst-webgui-framework'] },
```

Tailwind only scans the app's own sources, so the package's component
classes need one `@source` line in `style.css`:

```css
@source '../node_modules/@noob-audio-engineering/noob-vst-webgui-framework/crates/noob-vst-webgui-framework/web/vue';
```

Scoped styles that use `@apply` need `@reference '../style.css'`.

To work on the framework and a plug-in at the same time, link the package
instead of installing it: `npm link` in
`crates/noob-vst-webgui-framework/web` of this checkout, then
`npm link @noob-audio-engineering/noob-vst-webgui-framework` in the plug-in's
`web/`. A linked package is a symlink, so for the duration add
`resolve.preserveSymlinks: true` and the framework checkout to
`server.fs.allow` in the Vite config, and point the `@source` line at the
checkout. On the Rust side, a `[patch]` for the framework's git URL with
`path = "../noob-vst-webgui-framework/crates/<crate>"` entries does the
same.

## Test

```sh
cargo test --workspace                                  # 31 tests + 15 doctests
cargo test -p noob-vst-webgui-framework --test server   # the socket-level integration tests only
node tools/bench.mjs <port>                             # latency regression check against a running plug-in or standalone
node tools/play.mjs <port> 60 400                       # end-to-end audio smoke test against a synth
```

The integration tests in `crates/noob-vst-webgui-framework/tests/server.rs`
start a real server on an ephemeral port and drive it with a WebSocket
client; they cover the round trip, events, sticky streams, port probing, the
UI store, discovery, instance scoping and several clients at once. The
instance-scoping test probes every live instance on the machine, so it can
time out while several standalones are running; rerun it alone if so.

The plug-in repositories carry their own DSP tests and run them in their
own CI.

## Run and iterate

Clone a plug-in repository, build its SPA and run its standalone (its README
has the commands and the port), then, for UI work, use Vite's dev server
with hot reload from the plug-in's `web/`:

```sh
NOOB_VST_WEBGUI_FRAMEWORK_PORT=4242 npm run dev       # PowerShell: $env:NOOB_VST_WEBGUI_FRAMEWORK_PORT=4242; npm run dev
```

The dev server proxies `/ws` and `/instance*` to the standalone, so the page
at the Vite URL is live against the real engine. Rebuild `dist` before
building the plug-in. Before the plug-in exists, a page can run in offline
design mode (`configureClient({ offline })`, see the
[browser README](../crates/noob-vst-webgui-framework/web/README.md#offline-design-mode))
with a local manifest and synthetic frames.

## Conventions

* **Ids are stable API.** Parameter and stream ids (`b1_freq`,
  `spectrum_pre`, `filter_cutoff`) are used by the SPA, the presets, the
  tools and the tests. Renaming one is a breaking change; add, do not rename.
* **Normalized on the wire, plain at the edges.** Never send plain values.
* **Wire changes** need: `wire.rs`, the client decoder in
  `crates/noob-vst-webgui-framework/web/noob-vst-webgui-framework.js`,
  `docs/WIRE.md`, a test in `tests/server.rs`, and a `PROTOCOL_VERSION`
  bump if an old client could misread the new frames.
* **Real-time rule.** Anything reachable from `AudioHandle` must be
  wait-free or lock-free and must not allocate. Add a `# Real-time` note to
  its doc comment.
* **Filters in lockstep.** `crates/noob-vst-webgui-framework/web/components/eqcurve.js`
  draws the response the Rust filters in Noob-Q's
  [`src/dsp/filters.rs`](https://github.com/Noob-Audio-Engineering/noob-q/blob/main/src/dsp/filters.rs)
  produce. Change both, and compare visually against the sticky `curve`
  stream, which is the Rust side's own answer.
* **Sticky streams** for anything published on change.
* **Attribution.** Keep `authors`, `author`, `VENDOR` and manifest `vendor`
  consistent; the plug-ins ship with the vendor `Noob Audio Engineering`.
* **Formatting.** `cargo fmt --all` before committing; `cargo clippy
  --workspace --all-targets -- -D warnings` must be clean. There is no JS
  formatter configured; match the surrounding style (2-space indent, single
  quotes, trailing commas, 120-column lines).

## Documentation

* Rust API docs: `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace
  --document-private-items` must build clean (CI enforces it). Every public
  item has a doc comment; audio-thread APIs carry `# Real-time` notes.
* Browser API docs: JSDoc in the sources, reference in
  `crates/noob-vst-webgui-framework/web/README.md` and
  `crates/noob-vst-webgui-framework/web/components/README.md`.
* Guides live in `docs/` and are indexed by `docs/README.md`. When you add a
  feature, update the guide that owns it (protocol → `WIRE.md`, ports/store →
  `MULTI-INSTANCE.md`, timing → `PERFORMANCE.md`).
* The docs workflow (`.github/workflows/docs.yml`) builds rustdoc for the
  workspace and publishes it to
  [noob-audio-engineering.github.io/noob-vst-webgui-framework](https://noob-audio-engineering.github.io/noob-vst-webgui-framework/)
  on every push to `main`, with a redirect from the site root to
  `noob_vst_webgui_framework`. It needs Pages enabled with "GitHub Actions"
  as the source in the repository settings.

## CI

`.github/workflows/ci.yml` runs on every push and pull request, on Linux:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cargo doc` with warnings as errors
5. `node --check` over the tools and the browser package, and a check that
   the root `package.json` exports resolve
6. `cargo check -p noob-vst-webgui-framework-nih` in dev, in release and
   with the `devtools` feature, which compiles the patched nih-plug and
   catches the release-only wry gotcha

## Platform quirks

* **Windows: a running standalone `.exe` blocks `cargo build`** of that
  binary (the linker cannot replace a file that is executing). Kill it first,
  for example `taskkill /F /IM noob-q-standalone.exe`.
* **Windows: shell command length.** Scripts passed to a shell in one go are
  limited to about 8 KB by the process command line. Put longer scripts in a
  file.
* **Windows: `include_dir!` needs `web/dist`** at compile time for a
  plug-in's `plugin` feature; build the SPA first or the build fails with a
  confusing panic in the proc macro.
* **IDE keystrokes.** RustRover has occasionally inserted stray characters at
  the top of an open file when a tool changed it on disk. Check line 1 after
  external edits.
* **Ports.** If something else holds a standalone's port, it takes the next
  free one and prints it; `--port` insists. `node tools/instances.mjs` shows
  what is running.

## Release checklist

1. `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, `cargo doc` clean; the adapter checks in dev and
   release.
2. Build and run one plug-in against the new commit (`cargo update -p
   noob-vst-webgui-framework` in its repository), then `node tools/bench.mjs`
   and `node tools/play.mjs`.
3. Update `CHANGELOG.md`, bump `workspace.package.version` and the version in
   both `package.json` files, tag.
4. Nothing is on crates.io or npm yet: `noob-vst-webgui-framework-nih`
   depends on nih-plug from git, which crates.io rejects, and the plug-ins
   consume everything from git. The core and web view crates, and the browser
   package under the `@noob-audio-engineering` npm scope, can be published
   when wanted; the API docs go out through the docs workflow either way.
