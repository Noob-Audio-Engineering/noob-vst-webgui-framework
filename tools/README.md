# tools

Node scripts that talk to a running vst3-web-stratum instance. No dependencies; Node
20 or newer.

| Script | Purpose |
|---|---|
| `instances.mjs` | List the vst3-web-stratum servers running on this machine (discovery directory scan, or a server's `/instances`). |
| `bench.mjs <port>` | Ping round trip, edit echo latency, and per-stream rate / bandwidth / gap statistics. |
| `setparam.mjs <port> <id> <norm>` | Send one full parameter gesture. |
| `play.mjs <port> [note] [ms]` | Play a note through the events channel and check the output meter. |

Usage, sample output and how to read the numbers: [docs/TOOLS.md](../docs/TOOLS.md).
