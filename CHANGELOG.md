# Changelog

All notable changes to vst3-web-stratum. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
the workspace version in `Cargo.toml`.

## [Unreleased]

### Added

- A third example, Noob CompressorLab: two classic compressors in one
  plug-in, chosen per instance with a `model` parameter. The 1176 side is a
  FET compressor (feedback detector, ratio buttons with the all-buttons
  mode, every hardware revision from A to LN with its own circuit constants
  and faceplate look); the LA-2A side is an optical compressor (T4 cell
  model with the two-stage memory-dependent release, Compress / Limit,
  sidechain emphasis). Sourced research for both under `research/`, DSP
  tests, plug-in, standalone and one Vue + Tailwind SPA with both
  faceplates.
- Framework: `NeedleModel` (needle-meter ballistics and scale maths, no
  drawing), `Timeline` (scrolling history chart) and `LinePlot` (XY curve
  chart) canvas components; Vue `Timeline`, `LinePlot`, unstyled `Segmented`
  and `Toggle` controls; `useStreamValue`, `useStreamFrame` and `useNeedle`
  composables.
- Framework: live window resizing and fullscreen intent for plug-in pages
  (`useWindowSize`, the unstyled `ResizeGrip`, the `fullscreen` message and
  the `window` store key; the adapter applies resize requests without
  blocking behind other messages, remembers the size with the plug-in state
  and reopens at it), host-driven resizing (a frame drag in the host reaches
  the page: the adapter implements `Editor::can_resize`,
  `check_size_constraint` and `set_size`, which upstream nih-plug lacks, so
  the workspace builds against a patched fork through `[patch]`), an
  offline design mode for developing a page before
  its plug-in exists (`configureClient({ offline })`, `mockManifest`), and
  Vite configs that keep the linked framework out of the dependency
  pre-bundle so framework edits hot-reload.

## [0.1.0] - 2026-09-03

First complete version.

### Framework

- `vst3-web-stratum`: the bridge (`Vst3WebStratum`, `AudioHandle`), normalized
  parameters with linear / log / skew / table tapers, wait-free stream
  mailboxes with sticky replay, binary event frames, JSON messages, the UI
  store with size caps, a hand-rolled little-endian wire format
  (protocol version 1), an axum WebSocket / HTTP server with per-client
  subscriptions and full resync on overflow, `TCP_NODELAY`, port policies
  (fixed / ephemeral / probe), instance discovery (`/instance`,
  `/instances`, per-user discovery directory), and `FileStore` for
  standalone persistence.
- `vst3-web-stratum-webview`: the OS web view (WebView2 / WKWebView / WebKitGTK via
  `wry`) as a child of a host window, plus a native UI timer.
- `vst3-web-stratum-nih`: an nih-plug `Editor` that is the web view, parameter
  mirroring as 65-point tables, GUI-thread gesture forwarding, resize
  requests, name-hashed port probing, discovery, and `StoreSlot` persistence
  of the UI store in plug-in state.
- `@elyerinfox/vst3-web-stratum`: the browser client with zero-copy stream decoding,
  reconnect, parameter handles, gestures, history (undo / redo / A-B),
  events, the store; dependency-free canvas components (knob, meter,
  spectrum, EQ curve, scope, keyboard, wavetable, envelope); a Vue 3 layer
  with composables and components.

### Examples

- Noob-Q: a Pro-Q 4 style 24-band EQ (ten shapes, slopes to brickwall,
  dynamics, placement, zero / natural / linear phase, EQ match, spectrum
  grab, presets, A/B) with a nih-plug plug-in, a standalone, and a Vue +
  Tailwind SPA.
- Noob-Wave: a wavetable synth (mipmapped tables, unison, sub, SVF filter,
  two ADSRs, LFO, 16 voices) with MIDI, an on-screen keyboard, a standalone
  with real audio output, and a Vue + Tailwind SPA.

### Tooling and documentation

- `tools/`: latency bench, parameter setter, headless note player, instance
  lister.
- Guides in `docs/` (getting started, architecture, Rust API tour, wire
  protocol, multi-instance, performance, tools, development,
  troubleshooting), crate and package READMEs, rustdoc and JSDoc throughout.
- CI (fmt, clippy, test, doc, SPA builds) and a docs workflow publishing
  rustdoc to GitHub Pages.
