## Examples

I wrote the examples as humorous, affectionate spoofs of products I admire
(Noob-Q of FabFilter's Pro-Q; Noob-Wave of the classic wavetable synths),
to exercise the framework at product size. They are tributes, not parity
replacements, and I do not publish them.

| Document | Contents |# vst3-web-stratum documentation

Everything about the project, in reading order. The guides are for people
building a plug-in on vst3-web-stratum; the references are for looking things up.

## Guides

| Document | Read it when |
|---|---|
| [GETTING-STARTED.md](GETTING-STARTED.md) | You want to build your first plug-in with a browser-rendered UI, end to end. |
| [ARCHITECTURE.md](ARCHITECTURE.md) | You want to understand how the pieces fit: crates, threads, data flow, the real-time contract, and why things are the way they are. |
| [RUST-API.md](RUST-API.md) | You need a tour of the Rust API across `vst3-web-stratum`, `vst3-web-stratum-nih` and `vst3-web-stratum-webview` before diving into rustdoc. |
| [MULTI-INSTANCE.md](MULTI-INSTANCE.md) | You run several instances at once and care about ports, discovery and where UI state lives. |
| [PERFORMANCE.md](PERFORMANCE.md) | You want the latency numbers, how they were measured, and which knobs to turn. |
| [DEVELOPMENT.md](DEVELOPMENT.md) | You are working on this repository: layout, build, test, hot reload, conventions, CI. |
| [TROUBLESHOOTING.md](TROUBLESHOOTING.md) | Something does not work. |

## References

| Document | Contents |
|---|---|
| [WIRE.md](WIRE.md) | The wire protocol, byte by byte: binary frames, the manifest, text topics, the connect sequence, HTTP endpoints, ports. |
| [TOOLS.md](TOOLS.md) | The Node scripts in `tools/`: latency bench, set a parameter, play a note, list instances. |
| [../crates/vst3-web-stratum/README.md](../crates/vst3-web-stratum/README.md) | The bridge and server crate. |
| [../crates/vst3-web-stratum-nih/README.md](../crates/vst3-web-stratum-nih/README.md) | The nih-plug editor adapter. |
| [../crates/vst3-web-stratum-webview/README.md](../crates/vst3-web-stratum-webview/README.md) | Embedding the OS web view in a host window. |
| [../crates/vst3-web-stratum/web/README.md](../crates/vst3-web-stratum/web/README.md) | `@elyerinfox/vst3-web-stratum` and `@elyerinfox/vst3-web-stratum/vue`: the browser client, parameters, streams, store, history, Vue composables and components. |
| [../crates/vst3-web-stratum/web/components/README.md](../crates/vst3-web-stratum/web/components/README.md) | The dependency-free canvas components: knob, meter, spectrum, EQ curve, scope, keyboard, wavetable, envelope. |
| Rustdoc | `cargo doc --no-deps --workspace --open`, or the hosted copy published by the docs workflow (see [DEVELOPMENT.md](DEVELOPMENT.md#documentation)). |

## Examples

| Document | Contents |
|---|---|
| [../examples/noob-q/README.md](../examples/noob-q/README.md) | Noob-Q, the Pro-Q style EQ: DSP, parameters, streams, plug-in, standalone. |
| [../examples/noob-q/web/README.md](../examples/noob-q/web/README.md) | The Noob-Q Vue SPA. |
| [FEATURES.md](FEATURES.md) | Which Pro-Q 4 features Noob-Q implements, and how. |
| [PROQ4-FEATURES.md](PROQ4-FEATURES.md) | The feature inventory extracted from the Pro-Q 4 manual. |
| [../examples/noob-wave/README.md](../examples/noob-wave/README.md) | Noob-Wave, the wavetable synth: DSP, parameters, streams, plug-in, standalone. |
| [../examples/noob-wave/web/README.md](../examples/noob-wave/web/README.md) | The Noob-Wave Vue SPA. |

## Conventions used in these documents

* **Normalized** means a parameter value in `0..1`; **plain** means the value
  in its own unit (Hz, dB, %). The wire carries normalized values; pages
  convert with the taper or the 65-point table from the manifest.
* **Audio thread**, **pump thread** and **net thread** are the three threads
  on the plug-in side; see [ARCHITECTURE.md](ARCHITECTURE.md#threads).
* **Client** is one WebSocket connection: a plug-in window, a browser tab, a
  script. An **instance** is one running plug-in or standalone with its own
  server.
* File paths are relative to the repository root unless stated otherwise.
