# Development

How to work on this repository: layout, prerequisites, build and test
commands, the hot-reload workflow, conventions, documentation, CI, and the
platform quirks that cost time.

## Layout

```
Cargo.toml                 workspace: vst3-web-stratum, vst3-web-stratum-webview, vst3-web-stratum-nih, noob-q, noob-wave
crates/
  vst3-web-stratum/            bridge, params, streams, wire codec, server, discovery, store (Rust)
  vst3-web-stratum-webview/         wry wrapper: embed the OS web view, native UI timer (Rust)
  vst3-web-stratum-nih/             nih-plug Editor adapter, StoreSlot (Rust)
  noob-q/                  example 1: EQ.  src/dsp, src/plugin.rs, src/bin/standalone.rs, web/ (Vite SPA)
  noob-wave/               example 2: synth. same shape
crates/vst3-web-stratum/web/  @elyerinfox/vst3-web-stratum: vst3-web-stratum.js, components/, vue/, examples/vanilla (inside the core crate)
docs/                      guides and references (this folder)
tools/                     node scripts: bench, setparam, play, instances
.github/workflows/         ci.yml (fmt, clippy, test, doc, SPA builds), docs.yml (publish rustdoc)
```

The rule that shapes the tree: **generic code goes in the framework,
product code goes in the example crate.** If a component, composable, or
Rust helper could serve a different plug-in, it belongs in `crates/vst3-web-stratum/web/` or the
`vst3-web-stratum-*` crates. Each example crate owns its own Vite project under
`web/`; `crates/vst3-web-stratum/web/` is only the reusable library.

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
# framework and examples (default members: core, noob-q, noob-wave)
cargo build

# the standalone dev binaries, release mode (the numbers in PERFORMANCE.md are from release)
cargo build --release -p noob-q --bin noob-q-standalone -p noob-wave --bin noob-wave-standalone

# the SPAs (the standalones serve web/dist from disk; the plug-ins embed it)
(cd examples/noob-q/web && npm install && npm run build)
(cd examples/noob-wave/web && npm install && npm run build)

# the plug-ins (VST3 + CLAP cdylibs); web/dist must exist first
cargo build --release -p noob-q --features plugin
cargo build --release -p noob-wave --features plugin
```

Bundling into `.vst3` / `.clap` folders follows nih-plug's conventions; see
the README section "Build the plug-ins".

## Patched nih-plug

Upstream nih-plug has no host-to-plugin resizing: its VST3 view answers
`canResize` with "no" and its CLAP gui extension rejects `set_size`, so a
window could only grow when the plug-in asked. The workspace therefore
builds against my fork, branch `host-resize` of
`https://github.com/elyerinfox/nih-plug`, through a `[patch]` entry at the
bottom of the root `Cargo.toml`. The fork adds three `Editor` methods with
backwards-compatible defaults, `can_resize`, `check_size_constraint` and
`set_size`, and implements them in the VST3 and CLAP wrappers; the adapter in
`crates/vst3-web-stratum-nih` implements them for the web view. Everything
else is upstream as of commit `f36931f7`.

Dependents of the published crates need the same `[patch]` in their own
workspace until the change lands upstream, because Cargo applies patches
only from the root manifest. To move the workspace to a newer upstream,
rebase the branch on it, push, and run `cargo update -p nih_plug`.

## Test

```sh
cargo test -p vst3-web-stratum -p vst3-web-stratum-nih -p noob-q -p noob-wave   # 56 tests + doctests
cargo test -p vst3-web-stratum --test server                            # the socket-level integration tests only
node tools/play.mjs 4243 60 400                                     # end-to-end audio smoke test (standalone running)
node tools/bench.mjs 4242                                           # latency regression check
```

The integration tests in `crates/vst3-web-stratum/tests/server.rs` start a real
server on an ephemeral port and drive it with a WebSocket client; they cover
the round trip, events, sticky streams, port probing, the UI store,
discovery, and several clients at once.

## Run and iterate

```sh
./target/release/noob-q-standalone --open        # port 4242 or the next free one
./target/release/noob-wave-standalone --open     # port 4243, real audio through cpal (--silent to skip)
```

For UI work, run the standalone and use Vite's dev server with hot reload:

```sh
cd examples/noob-q/web
VST3_WEB_STRATUM_PORT=4242 npm run dev       # PowerShell: $env:VST3_WEB_STRATUM_PORT=4242; npm run dev
```

The dev server proxies `/ws` and `/instance*` to the standalone, so the page
at the Vite URL is live against the real engine. Rebuild `dist` before
building the plug-in.

`@elyerinfox/vst3-web-stratum` is linked into each SPA with `"file:../../../crates/vst3-web-stratum/web"`. Vite
needs `resolve.preserveSymlinks: true`, `dedupe: ['vue']` (so the library's
Vue layer uses the app's Vue), `server.fs.allow` including the repo root, and
Tailwind needs `@source '../../../../crates/vst3-web-stratum/web/vue'` in `style.css` to see the
library's component classes. Scoped styles that use `@apply` need
`@reference '../style.css'`.

## Conventions

* **Ids are stable API.** Parameter and stream ids (`b1_freq`,
  `spectrum_pre`, `filter_cutoff`) are used by the SPA, the presets, the
  tools and the tests. Renaming one is a breaking change; add, do not rename.
* **Normalized on the wire, plain at the edges.** Never send plain values.
* **Wire changes** need: `wire.rs`, the client decoder in `crates/vst3-web-stratum/web/vst3-web-stratum.js`,
  `docs/WIRE.md`, a test in `tests/server.rs`, and a `PROTOCOL_VERSION`
  bump if an old client could misread the new frames.
* **Real-time rule.** Anything reachable from `AudioHandle` must be
  wait-free or lock-free and must not allocate. Add a `# Real-time` note to
  its doc comment.
* **Filters in lockstep.** `crates/vst3-web-stratum/web/components/eqcurve.js` draws the response
  the Rust filters in `examples/noob-q/src/dsp/filters.rs` produce. Change
  both, and compare visually against the sticky `curve` stream, which is the
  Rust side's own answer.
* **Sticky streams** for anything published on change.
* **Attribution.** Keep `authors`, `author`,
  `VENDOR` and manifest `vendor` consistent.
* **Formatting.** `cargo fmt --all` before committing; `cargo clippy
  --workspace --all-targets -- -D warnings` must be clean. There is no JS
  formatter configured; match the surrounding style (2-space indent, single
  quotes, trailing commas, 120-column lines).

## Documentation

* Rust API docs: `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace
  --document-private-items` must build clean (CI enforces it). Every public
  item has a doc comment; audio-thread APIs carry `# Real-time` notes.
* Browser API docs: JSDoc in the sources, reference in `crates/vst3-web-stratum/web/README.md` and
  `crates/vst3-web-stratum/web/components/README.md`.
* Guides live in `docs/` and are indexed by `docs/README.md`. When you add a
  feature, update the guide that owns it (protocol → `WIRE.md`, ports/store →
  `MULTI-INSTANCE.md`, timing → `PERFORMANCE.md`).
* The docs workflow (`.github/workflows/docs.yml`) builds rustdoc for the
  workspace and publishes it to GitHub Pages on every push to `main`, with a
  redirect from the site root to `vst3_web_stratum`. It needs Pages enabled with
  "GitHub Actions" as the source in the repository settings.

## CI

`.github/workflows/ci.yml` runs on every push and pull request, on Linux:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test` for every crate
4. `cargo doc` with warnings as errors
5. `npm ci && npm run build` for both SPAs, then the plug-in feature check,
   which needs `web/dist`

## Platform quirks

* **Windows: a running standalone `.exe` blocks `cargo build`** of that
  binary (the linker cannot replace a file that is executing). Kill it first:
  `taskkill /F /IM noob-q-standalone.exe`.
* **Windows: shell command length.** Scripts passed to a shell in one go are
  limited to about 8 KB by the process command line. Put longer scripts in a
  file.
* **Windows: `include_dir!` needs `web/dist`** at compile time for the
  `plugin` feature; build the SPA first or the build fails with a confusing
  panic in the proc macro.
* **IDE keystrokes.** RustRover has occasionally inserted stray characters at
  the top of an open file when a tool changed it on disk. Check line 1 after
  external edits.
* **Ports.** If something else holds 4242, the standalone takes the next
  free port and prints it; `--port` insists. `node tools/instances.mjs` shows
  what is running.

## Release checklist

1. `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test`, `cargo doc` clean.
2. Build both SPAs, then both plug-ins with `--features plugin`.
3. Run both standalones; `node tools/bench.mjs` and `node tools/play.mjs`.
4. Update `CHANGELOG.md`, bump `workspace.package.version`, tag.
5. The crates cannot be published to crates.io as they are: `vst3-web-stratum-nih`
   depends on nih-plug from git (crates.io rejects git dependencies) and the
   name `vst3-web-stratum` is already taken there. Publish the API docs through
   the docs workflow instead, or rename before publishing.
