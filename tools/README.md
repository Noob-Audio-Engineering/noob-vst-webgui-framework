# tools

Node scripts that talk to a running noob-vst-webgui-framework instance. No dependencies; Node
20 or newer.

| Script | Purpose |
|---|---|
| `instances.mjs` | List the noob-vst-webgui-framework servers running on this machine (discovery directory scan, or a server's `/instances`). |
| `bench.mjs <port>` | Ping round trip, edit echo latency, and per-stream rate / bandwidth / gap statistics. |
| `setparam.mjs <port> <id> <norm>` | Send one full parameter gesture. |
| `play.mjs <port> [note] [ms]` | Play a note through the events channel and check the output meter. |

Usage, sample output and how to read the numbers: [docs/TOOLS.md](../docs/TOOLS.md).
