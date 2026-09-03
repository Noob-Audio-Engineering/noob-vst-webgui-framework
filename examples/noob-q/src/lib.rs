//! # noob-q
//!
//! A Pro-Q style 24-band parametric EQ, the first example built on
//! vst3-web-stratum: the plug-in's editor is the operating system's web view,
//! rendering a Vue + Tailwind single-page app that talks to this crate's
//! DSP over a local WebSocket. Everything that is "an EQ" lives here;
//! everything reusable lives in the framework crates (`vst3-web-stratum`,
//! `vst3-web-stratum-nih`, `vst3-web-stratum-webview`) and the browser library
//! (`@elyerinfox/vst3-web-stratum`).
//!
//! ## What the example demonstrates
//!
//! * A large parameter set mirrored to the page with stable ids, groups,
//!   units, tapers and labels: 21 global parameters plus 24 bands × 15
//!   parameters (381 ids; the standalone adds 5 demo-source ids).
//! * Eight telemetry streams at different rates and of different kinds:
//!   three spectra, two stereo meters, a sticky response curve, and per-band
//!   dynamic gain and detector level.
//! * Real-time DSP with zero-latency IIR, "natural phase" and true
//!   linear-phase (FFT convolution) modes, per-band dynamics with an
//!   external side-chain, mid/side placement, solo, auto gain and
//!   saturation.
//! * Plug-in state that carries the page's own data (user presets,
//!   favourites, EQ Match references) through vst3-web-stratum's UI store.
//! * A standalone binary so the SPA can be developed in a normal browser
//!   without a DAW, with the same ids and streams as the plug-in.
//!
//! ## Layout
//!
//! | Part | Where | Role |
//! |---|---|---|
//! | DSP | [`dsp`] | Filters, dynamics, convolver, analyzer, engine, demo sources, and the parameter / stream layout. Knows nothing about hosts or sockets. |
//! | Plug-in | `plugin` (feature `plugin`) | nih-plug VST3 / CLAP effect. Owns the parameters, feeds the engine from `process`, publishes the streams, embeds `web/dist`. |
//! | Standalone | `src/bin/standalone.rs` | A fake audio thread on demo signals plus the vst3-web-stratum server, for UI work without a DAW. |
//! | SPA | `web/` | The interface (Vue 3, Tailwind, Vite), built to `web/dist`. Documented in `crates/vst3-web-stratum/web/README.md`. |
//!
//! The DSP and the parameter layout are shared: [`dsp::param_specs`] and
//! the plug-in's hand-written `Params` implementation produce the same ids
//! in the same order, so one SPA serves both the plug-in and the standalone.
//!
//! ## Where the framework ends
//!
//! vst3-web-stratum provides the bridge (parameters, streams, events, messages, the
//! UI store), the server, the editor adapter and the browser client with
//! generic components (knob, spectrum, EQ curve, meter). This crate adds
//! the EQ: filter design, the processing chain, what each stream contains,
//! and the Pro-Q-shaped page. The rule used throughout the repository: if
//! something here would be useful to a second plug-in, it belongs in the
//! framework instead.
//!
//! ## Features
//!
//! * `plugin` — builds the nih-plug plug-in and the VST3 / CLAP exports.
//!   Requires `web/dist` to exist, because the SPA is embedded at compile
//!   time. Off by default so `cargo test` and the standalone need neither
//!   nih-plug nor a built SPA.
//!
//! See `README.md` in this crate for the parameter and stream tables, and
//! `docs/FEATURES.md` at the repository root for coverage against the
//! Pro-Q 4 manual.

// DSP loops index several buffers by the same sample or bin index; iterator
// chains would hide the arithmetic the comments describe.
#![allow(clippy::needless_range_loop)]

pub mod dsp;

#[cfg(feature = "plugin")]
pub mod plugin;

// The VST3 and CLAP entry points. nih-plug generates the C ABI exports from
// the `Plugin` / `Vst3Plugin` / `ClapPlugin` impls in `plugin.rs`.
#[cfg(feature = "plugin")]
nih_plug::nih_export_vst3!(plugin::NoobQ);
#[cfg(feature = "plugin")]
nih_plug::nih_export_clap!(plugin::NoobQ);
