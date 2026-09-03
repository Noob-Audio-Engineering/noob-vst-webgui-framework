# noob-vst-webgui-framework documentation

Everything about the project, in reading order. The guides are for people
building a plug-in on noob-vst-webgui-framework; the references are for looking things up.

## Guides

| Document | Read it when |
|---|---|
| [GETTING-STARTED.md](GETTING-STARTED.md) | You want to build your first plug-in with a browser-rendered UI, end to end. |
| [ARCHITECTURE.md](ARCHITECTURE.md) | You want to understand how the pieces fit: crates, threads, data flow, the real-time contract, and why things are the way they are. |
| [RUST-API.md](RUST-API.md) | You need a tour of the Rust API across `noob-vst-webgui-framework`, `noob-vst-webgui-framework-nih` and `noob-vst-webgui-framework-webview` before diving into rustdoc. |
| [MULTI-INSTANCE.md](MULTI-INSTANCE.md) | You run several instances at once and care about ports, discovery and where UI state lives. |
| [PERFORMANCE.md](PERFORMANCE.md) | You want the latency numbers, how they were measured, and which knobs to turn. |
| [DEVELOPMENT.md](DEVELOPMENT.md) | You are working on this repository, or on a plug-in that depends on it: layout, build, test, hot reload, conventions, CI. |
| [TROUBLESHOOTING.md](TROUBLESHOOTING.md) | Something does not work. |

## References

| Document | Contents |
|---|---|
| [WIRE.md](WIRE.md) | The wire protocol, byte by byte: binary frames, the manifest, text topics, the connect sequence, HTTP endpoints, ports. |
| [TOOLS.md](TOOLS.md) | The Node scripts in `tools/`: latency bench, set a parameter, play a note, list instances. |
| [../crates/noob-vst-webgui-framework/README.md](../crates/noob-vst-webgui-framework/README.md) | The bridge and server crate. |
| [../crates/noob-vst-webgui-framework-nih/README.md](../crates/noob-vst-webgui-framework-nih/README.md) | The nih-plug editor adapter. |
| [../crates/noob-vst-webgui-framework-webview/README.md](../crates/noob-vst-webgui-framework-webview/README.md) | Embedding the OS web view in a host window. |
| [../crates/noob-vst-webgui-framework/web/README.md](../crates/noob-vst-webgui-framework/web/README.md) | `@noob-audio-engineering/noob-vst-webgui-framework` and `@noob-audio-engineering/noob-vst-webgui-framework/vue`: the browser client, parameters, streams, store, history, Vue composables and components. |
| [../crates/noob-vst-webgui-framework/web/components/README.md](../crates/noob-vst-webgui-framework/web/components/README.md) | The dependency-free canvas components: knob, meter, spectrum, EQ curve, scope, keyboard, wavetable, envelope, history and curve charts. |
| Rustdoc | `cargo doc --no-deps --workspace --open`, or the hosted copy at [noob-audio-engineering.github.io/noob-vst-webgui-framework](https://noob-audio-engineering.github.io/noob-vst-webgui-framework/), published by the docs workflow (see [DEVELOPMENT.md](DEVELOPMENT.md#documentation)). |

## Plug-ins built on it

Noob Audio Engineering publishes three free plug-ins on the framework, each
in its own repository with its own documentation. I wrote them as humorous,
affectionate spoofs of products I admire (Noob-Q of FabFilter's Pro-Q;
Noob-Wave of the classic wavetable synths; Noob CompressorLab of the UREI
1176 and the Teletronix LA-2A), to exercise the framework at product size.
They are tributes, not parity replacements.

| Repository | Contents |
|---|---|
| [noob-q](https://github.com/Noob-Audio-Engineering/noob-q) | Noob-Q, the Pro-Q style EQ: DSP, parameters, streams, plug-in, standalone, the Vue SPA (`web/README.md`), and `docs/FEATURES.md` with `docs/PROQ4-FEATURES.md` on which Pro-Q 4 features it implements and how. |
| [noob-wave](https://github.com/Noob-Audio-Engineering/noob-wave) | Noob-Wave, the wavetable synth: DSP, parameters, streams, plug-in, standalone, the Vue SPA. |
| [noob-compressorlab](https://github.com/Noob-Audio-Engineering/noob-compressorlab) | Noob CompressorLab, the 1176-style FET and LA-2A-style optical compressors in one plug-in: DSP, parameters, streams, plug-in, standalone, the Vue SPA with the model switch and both faceplates, and `research/1176.md` with `research/LA-2A.md` on how the originals work and how they are simulated. |

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
