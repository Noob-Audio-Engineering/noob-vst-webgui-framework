# Changelog

All notable changes to noob-vst-webgui-framework. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
the workspace version in `Cargo.toml`.

## [Unreleased]

### Fixed

- A stepped parameter's displayed value is rounded to its own step rather
  than to an integer. The step is `(max - min) / (steps - 1)` and nothing
  requires that to be 1: a half-decibel switch is ordinary hardware, and two
  neighbouring detents were rendering as the same string, so a host showed a
  value that did not change when the user stepped it. Every integer-stepped
  control renders exactly as before.

- `EqCurve` cascaded a steep shelf as `n` identical sections carrying
  `gain/n`, which sums to one shelf of the full gain at no extra steepness,
  so a drawn shelf ignored its slope control. Sections now take the
  Butterworth Qs of the combined order `2n`, a band's Q maps onto a shelf's
  through the new `shelfQ` and each section is held to a ceiling from its
  own gain, so the cookbook form cannot be driven onto the unit circle. For
  a cut, the band's Q now shapes the most resonant section rather than the
  least, and Q is locked at the 6 dB/oct slope. A drawn curve was
  disagreeing with the audio by up to 1.87 dB.

### Added

- `Timeline` series take a `peaks` option that marks the moments a series
  peaked and names their values, so a history chart says how deep the worst
  of them went without the reader tracing the scale. Peaks are found as the
  samples arrive rather than by scanning at draw time, so one marks a genuine
  local extreme and not whichever sample happened to be lowest, and each
  rides at its own moment so it scrolls off the chart with its peak. Each is
  a dot with its value in a callout box whose pointer aims back at it, drawn
  as one path so the outline has no seam and flipped to the other side of the
  dot when there is no room. The set sits at `dimOpacity` so as not to shout
  and comes to full strength while the pointer is over the chart. Off by
  default, allocating nothing and costing nothing when off, down to adding no
  pointer listeners; the dot, the box and its text take the series' own
  colour and the caller's formatter, so the component decides nothing about
  how the value reads.
- `Timeline` takes `timeGrid`, which runs each second's mark the whole height
  of the chart instead of a stub at the bottom, so peaks can be read against
  the time axis.
- The browser client keeps `manifest.meta.sample_rate` current from the
  plug-in's `sample_rate` message, as the Vue layer already did. A manifest
  is built before a plug-in knows its rate, so a page that read it once put
  every spectrum peak at half its true frequency at 96 kHz.
- `EqCurve` takes a `bandQMax` option, the top of the plug-in's own band-Q
  range that the shelf-Q compression is scaled against. It was assumed to be
  40, which is what the equaliser here uses, and one with a different range
  would have drawn a shelf that disagreed with its own audio.
- `EqCurve` takes an `offsetDb` option and a `setOffsetDb` setter: a
  constant offset on the composite curve for a global make-up or auto gain,
  leaving the band curves and node handles alone. Without it a page has to
  transform the component's own paths, which an equaliser here was doing.
- Debug builds assert that a published stream frame holds only finite
  values. A NaN or an infinity means the plug-in's processing has come
  apart, and passing it on only produces a blank meter with no explanation;
  release builds do not check, so it costs nothing shipped.

- A third plug-in, Noob CompressorLab: two classic compressors in one
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

### Changed

- Renamed from vst3-web-stratum to noob-vst-webgui-framework and moved to
  the Noob Audio Engineering organisation: the crates
  `noob-vst-webgui-framework`, `noob-vst-webgui-framework-nih` and
  `noob-vst-webgui-framework-webview`, the browser package
  `@noob-audio-engineering/noob-vst-webgui-framework`, and every identifier,
  route, CSS variable, environment variable and store key with them
  (`NoobVstWebguiFramework*`, `useNoobVstWebguiFramework`,
  `/noob-vst-webgui-framework/`, `--noob-vst-webgui-framework-*`,
  `NOOB_VST_WEBGUI_FRAMEWORK_PORT`, `noob_vst_webgui_framework_ui_store`).
- The plug-ins left this repository for their own: Noob-Q, Noob-Wave and
  Noob CompressorLab are free plug-ins published by Noob Audio Engineering,
  each depending on the framework crates from git and on the browser
  package installed from this repository, which a root `package.json`
  re-exports. Noob-Q's feature coverage documents went with it.
- The nih-plug fork with host-driven resizing moved to
  `Noob-Audio-Engineering/nih-plug`.

## [0.1.0] - 2026-09-03

First complete version.

### Framework

- `noob-vst-webgui-framework`: the bridge (`NoobVstWebguiFramework`, `AudioHandle`), normalized
  parameters with linear / log / skew / table tapers, wait-free stream
  mailboxes with sticky replay, binary event frames, JSON messages, the UI
  store with size caps, a hand-rolled little-endian wire format
  (protocol version 1), an axum WebSocket / HTTP server with per-client
  subscriptions and full resync on overflow, `TCP_NODELAY`, port policies
  (fixed / ephemeral / probe), instance discovery (`/instance`,
  `/instances`, per-user discovery directory), and `FileStore` for
  standalone persistence.
- `noob-vst-webgui-framework-webview`: the OS web view (WebView2 / WKWebView / WebKitGTK via
  `wry`) as a child of a host window, plus a native UI timer.
- `noob-vst-webgui-framework-nih`: an nih-plug `Editor` that is the web view, parameter
  mirroring as 65-point tables, GUI-thread gesture forwarding, resize
  requests, name-hashed port probing, discovery, and `StoreSlot` persistence
  of the UI store in plug-in state.
- `@noob-audio-engineering/noob-vst-webgui-framework`: the browser client with zero-copy stream decoding,
  reconnect, parameter handles, gestures, history (undo / redo / A-B),
  events, the store; dependency-free canvas components (knob, meter,
  spectrum, EQ curve, scope, keyboard, wavetable, envelope); a Vue 3 layer
  with composables and components.

### Plug-ins

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
