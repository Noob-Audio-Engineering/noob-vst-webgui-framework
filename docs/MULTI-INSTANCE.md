# Multiple instances

A DAW session loads the same plug-in many times, and a developer runs a
standalone next to a plug-in next to a browser tab. Every instance runs its
own server, so three questions need an answer: which port, how do you find
an instance, and where does page state live so that it follows the plug-in
rather than the browser. This document covers all three.

## Ports

A server binds `127.0.0.1` under one of three policies (`PortPolicy` in
`vst3-web-stratum`):

| Policy | Behaviour | Use it when |
|---|---|---|
| `Fixed(port)` | Bind exactly that port; fail if it is taken. | A tool insists on one port (`--port N` in the standalones). |
| `Ephemeral` | Let the OS pick a free port. | You never need a stable origin and want zero chance of collision. |
| `Probe { base, span }` | Try `base`, `base+1`, … `base+span-1`; take the first free one. | Everything else. This is the default for plug-ins and standalones. |

Plug-ins default to `PortPolicy::for_name(name)`: an FNV-1a hash of the
plug-in name selects a base in `49152..64151` (the dynamic port range) and
the span is 64. Two different plug-ins therefore probe different ranges, and
up to 64 instances of the same plug-in coexist. The first instance of a
plug-in usually gets the same port every session, which keeps the page's
origin (`http://127.0.0.1:<port>`) stable, which in turn keeps whatever the
browser engine stores per origin (its own `localStorage`, caches, devtools
settings) attached to that plug-in.

Standalones prefer their documented port (4242 for Noob-Q, 4243 for
Noob-Wave) and walk up from there, so a second copy takes 4244 rather than
panicking. `--port N` switches to `Fixed(N)`.

In code:

```rust
use vst3_web_stratum::{PortPolicy, ServerConfig};

ServerConfig::default().port(4242);                 // Fixed(4242); 0 means Ephemeral
ServerConfig::default().prefer_port(4242);          // Probe { base: 4242, span: 32 }
ServerConfig::default().ephemeral();
ServerConfig::default().port_policy(PortPolicy::for_name("My Plug-in"));
```

With the nih-plug adapter, `EditorConfig::new(w, h)` already uses the name
hash; `.port(n)` and `.port_policy(p)` override it.

## Discovery

Every server (unless `ServerConfig::discovery(false)`) writes a JSON record
on start and removes it on a clean stop:

```
%LOCALAPPDATA%\vst3-web-stratum\instances\<pid>-<port>.json            Windows
~/Library/Application Support/vst3-web-stratum/instances/<pid>-<port>.json   macOS
$XDG_RUNTIME_DIR/vst3-web-stratum/instances/<pid>-<port>.json           Linux (falls back to ~/.local/state)
```

```json
{ "name": "noob-q", "pid": 34080, "port": 4242, "url": "http://127.0.0.1:4242/", "started": 1788397204, "protocol": 1 }
```

The same record is served at `GET /instance` by every server. `GET
/instances` returns the records of the **live** instances of the **same
plug-in** (same `name`) on the machine: each file is read, its port is probed
with a short timeout, records whose server does not answer (a crashed
process) are deleted, and records of other plug-ins are left out. A record
is trusted only if the answering server reports the same pid, so a port
reused by an unrelated program is not mistaken for an instance. Instance
features are scoped to one product on purpose: an EQ's instance list should
show the other copies of that EQ, not every vst3-web-stratum app on the machine.
`GET /instances?all=1` lifts the restriction for tooling.

From the shell, `node tools/instances.mjs` scans everything without a
running server (`--name noob-q` narrows it), and `node tools/instances.mjs
4242` asks a running server for its own plug-in's instances (`--all` for
every instance). The Noob-Q instance button (bottom centre) lists the other
live Noob-Q instances and opens them in a new window.

The Rust API is in `vst3_web_stratum::discovery`: `Instance`, `dir()`,
`publish`, `unpublish`, `list_files`, `probe`, `list_live`.

## The UI store

### What it is for

Pages keep state that is not a parameter: user presets, favourites, saved
reference spectra, which panel is open, the chosen window size. Before the
store existed that went to `localStorage`, which is tied to the browser
profile and the origin: it did not follow the plug-in into another session,
it was invisible to a second window of the same instance, and with ephemeral
ports it vanished every restart.

The store is a JSON object owned by the plug-in. Every client of the
instance sees the same values, changes fan out immediately, and the plug-in
persists it with its own state, so a preset list saved in a DAW session comes
back with that session. The browser's own storage remains available for
per-machine conveniences that should not travel (a collapsed help panel, a
devtools preference).

### Semantics

* Keys are strings, values are any JSON. Setting a key to `null` removes it.
* On connect the server sends `store.all` with every key after the sticky
  stream frames, so a page can render presets before the first telemetry
  frame arrives.
* `store.set` from a client updates the plug-in's copy and is forwarded as
  `store.changed` to every **other** client. The sender applies its own
  change locally and does not get an echo.
* When the plug-in replaces the whole store (a host restoring state, a file
  loaded by a standalone) every client gets a fresh `store.all`.
* Last writer wins. There is no merging; keep values small and whole (a
  preset list, not one preset per key, unless you want per-key granularity).
* Limits: a single value may be at most 256 KiB and the whole store at most
  1 MiB. A rejected `store.set` is answered with `store.error` to the sender
  only, and the store is unchanged.

The exact frames are in [WIRE.md](WIRE.md#reserved-topics-the-ui-store).

### In the browser

```js
const client = new Vst3WebStratumClient();
client.store.ready;                       // true once store.all arrived
client.store.get('presets.user', []);     // cached read, default if absent
client.store.set('presets.user', list);   // local cache + store.set to the plug-in
client.store.on('presets.user', (v) => render(v));
client.store.on('*', (key, value) => { if (key === null) rehydrated(); });
```

With Vue, `useStore()` returns `{ ready, data, get, set }` where `data` is a
reactive object of every key, and `useStoredRef(key, dflt)` is a writable
computed bound to one key.

### In the plug-in

`Vst3WebStratum` exposes `store_get`, `store_set`, `store_snapshot`, `store_json`,
`store_replace`, `store_load_json` and `set_store_hook` (called on every
change, on the net thread). Two helpers cover persistence:

* **`vst3_web_stratum_nih::StoreSlot`** for plug-ins. Put one in your `Params` struct,
  call `attach(&bridge)` once the bridge exists, and forward
  `serialize_fields` / `deserialize_fields` to it. It stores the JSON under
  the key `vst3_web_stratum_ui_store` in the plug-in's persistent fields. State the
  host restores before `attach` is applied when `attach` is called, so
  construction order does not matter. A restored state without the key
  empties the store, which is the right outcome for a state saved before the
  page kept anything.
* **`vst3_web_stratum::FileStore`** for standalones. `FileStore::attach(&vst3-web-stratum,
  path)` loads the file (a missing file is an empty store) and marks the
  store dirty on every change; `flush()` from the host loop writes it back
  atomically (temp file plus rename) only when something changed.
  `FileStore::default_path(name)` is `<data dir>/vst3-web-stratum/<name>.store.json`,
  next to the discovery records. Two standalone copies of the same program
  share that file, last writer wins.

### What the examples put there

| Key | Owner | Value |
|---|---|---|
| `presets.user` | both | the user's preset list `[{ name, values, ... }]` |
| `presets.favorites` | Noob-Q | array of favourite preset names |
| `eqmatch.references` | Noob-Q | saved reference spectra `[{ name, data: number[128] }]` |

## Several windows of one instance

Because the store, the parameter values and the sticky streams are all
server-side, a second window (a browser tab pointed at the instance's URL,
or a detached window opened from the instance menu) shows the same state as
the plug-in window and edits from either are seen by the other within one
pump cycle. Each client has its own subscriptions and throttles, so a
background tab can turn its spectra off without affecting the plug-in window.

## Checklist for your own plug-in

1. Keep the default port policy unless you have a reason; it needs no
   configuration.
2. Leave discovery on so tools can find your instances; turn it off only if
   writing to the user's data directory is unacceptable.
3. Put state that belongs to the plug-in in the store and persist it with a
   `StoreSlot`. Keep values whole and under the limits.
4. Use the browser's storage only for things that should stay on this
   machine and this browser profile.
5. Never assume a fixed port in the page: connect to `location.host` (the
   default) and let the server tell you everything else in the manifest.
