//! noob-q-standalone: the example without a DAW.
//!
//! A fake audio thread runs two demo sources (main input and side-chain)
//! through the engine at 48 kHz in 256-sample blocks, paced by the wall
//! clock, publishes exactly the streams the plug-in publishes (spectra,
//! meters, the response curve, per-band dynamic gain and level), and serves
//! the SPA from `web/dist` over vst3-web-stratum. It exists so the page can be
//! developed and benchmarked in a normal browser; the `source` parameter
//! group (present only here, with `standalone: true` in the manifest meta)
//! picks the demo signals.
//!
//! ```text
//! cargo run -p noob-q --bin noob-q-standalone -- [--port N] [--open] [--dir path]
//! ```
//!
//! * `--port N` insists on port `N`. Without it the server starts at 4242
//!   and walks up to the next free port, so a second copy (or the synth
//!   example on 4243) never collides; the actual URL is printed.
//! * `--open` launches the system browser on that URL.
//! * `--dir path` serves another asset directory. The default is `web/dist`;
//!   when it does not exist the binary prints how to build it, or how to run
//!   `npm run dev` with `VST3_WEB_STRATUM_PORT` set so Vite proxies `/ws` here.
//! * `RUST_LOG=debug` logs every edit gesture that arrives from the page.
//!
//! State the page keeps in `client.store` (user presets, favourites, EQ
//! Match references) persists in `<per-user data dir>/vst3-web-stratum/noob-q.store.json`
//! through vst3-web-stratum's `FileStore`; the plug-in keeps the same data inside its
//! host state instead.
//!
//! The host loop plays the part a DAW would: it drains edit gestures (a DAW
//! would write them into automation), answers the page's `reset` message by
//! restoring every default, ignores `resize` (meaningful only in a plug-in
//! window), and once a second sends a `status` message with `{ clients,
//! blocks, edits, dropped, sample_rate, block, latency_samples, latency_ms }`
//! that the page shows in its status line.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use noob_q::dsp::{
    self, Analyzer, BANDS, BandSettings, CURVE_MAX_HZ, CURVE_MIN_HZ, CURVE_POINTS, Engine,
    MAX_BINS, Meter, ParamIx, STREAM_IX, Source,
};
use serde_json::json;
use vst3_web_stratum::{AudioHandle, FileStore, ServerConfig, Vst3WebStratum};

/// Sample rate of the fake audio thread.
const SR: f32 = 48_000.0;
/// Block size of the fake audio thread (5.3 ms at 48 kHz, a typical DAW
/// buffer; also the telemetry rate).
const BLOCK: usize = 256;

/// Counters the audio thread exposes to the host loop for the status line.
struct Stats {
    /// Blocks rendered so far.
    blocks: AtomicU64,
    /// The engine's current latency, samples.
    latency: AtomicUsize,
}

/// The stand-in for a DAW's audio callback: an endless loop rendering one
/// block per `BLOCK / SR` seconds. Per block it reads the parameters into
/// settings, generates the demo signals, runs the analyzers, meters and the
/// engine exactly as the plug-in does, and publishes the streams at the
/// plug-in's rates. Never allocates after the buffers below are made.
fn audio_thread(mut audio: AudioHandle, p: ParamIx, stats: Arc<Stats>) {
    let mut engine = Engine::new(SR);
    let mut source = Source::new(0x9E37_79B9);
    let mut sc_source = Source::new(0x1234_5678);
    let mut an_pre = Analyzer::new();
    let mut an_post = Analyzer::new();
    let mut an_sc = Analyzer::new();
    let mut meter_in = Meter::default();
    let mut meter_out = Meter::default();
    let mut bands = [BandSettings::default(); BANDS];
    let mut spectrum = vec![0.0f32; MAX_BINS];
    let mut curve = vec![0.0f32; CURVE_POINTS];
    let mut band_dyn = vec![0.0f32; BANDS];
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    let mut scl = vec![0.0f32; BLOCK];
    let mut scr = vec![0.0f32; BLOCK];
    let block_dur = Duration::from_secs_f64(BLOCK as f64 / SR as f64);
    let mut next = Instant::now();
    let mut n: u64 = 0;
    let mut last_resolution = usize::MAX;

    loop {
        // 1. Parameters → settings. `configure` says whether the static
        //    response (and so the curve stream) changed.
        for (b, ix) in bands.iter_mut().zip(&p.bands) {
            *b = dsp::read_band(&audio, ix);
        }
        let g = dsp::read_globals(&audio, &p);
        let changed = engine.configure(&bands, g);
        let src_kind = audio.param(p.src_kind).round() as usize;
        let src_freq = audio.param(p.src_freq);
        let src_level = audio.param(p.src_level);
        let sc_kind = audio.param(p.sc_kind).round() as usize;
        let sc_level = audio.param(p.sc_level);
        let want_pre = audio.param(p.analyzer_pre) >= 0.5;
        let want_post = audio.param(p.analyzer_post) >= 0.5;
        let want_sc = audio.param(p.analyzer_sc) >= 0.5;
        let resolution = audio.param(p.analyzer_resolution).round() as usize;
        if resolution != last_resolution {
            last_resolution = resolution;
            an_pre.set_resolution(resolution);
            an_post.set_resolution(resolution);
            an_sc.set_resolution(resolution);
        }

        // 2. Generate: the main source is slightly asymmetric between
        //    channels (and carries a little of the side-chain signal) so
        //    mid/side placement has something to work with. Pre analyzer and
        //    input meter see the generated signal; the side-chain analyzer
        //    sees the side-chain source.
        for i in 0..BLOCK {
            let x = source.next(src_kind, src_freq, SR) * src_level;
            let s = sc_source.next(sc_kind, 110.0, SR) * sc_level;
            l[i] = x;
            r[i] = x * 0.92 + s * 0.05;
            scl[i] = s;
            scr[i] = s;
            an_pre.push(0.5 * (l[i] + r[i]));
            an_sc.push(s);
            meter_in.feed(l[i], r[i]);
        }
        // 3. The EQ, with the side-chain source as the external detector
        //    input, then the post analyzer and output meter.
        engine.process_block(&mut l, &mut r, Some((&scl, &scr)));
        for i in 0..BLOCK {
            an_post.push(0.5 * (l[i] + r[i]));
            meter_out.feed(l[i], r[i]);
        }
        n += 1;
        stats.blocks.store(n, Ordering::Relaxed);
        stats.latency.store(engine.latency(), Ordering::Relaxed);

        // 4. Telemetry at the plug-in's rates: meters and dynamic gains every
        //    block, detector levels every 4th, spectra every 2nd (only the
        //    ones the page asked for), the curve when it changed.
        audio.publish_slice(STREAM_IX.meter_in, &meter_in.take());
        audio.publish_slice(STREAM_IX.meter_out, &meter_out.take());
        engine.band_dyn_gains(&mut band_dyn);
        audio.publish_slice(STREAM_IX.band_dyn, &band_dyn);
        if n.is_multiple_of(4) {
            engine.band_levels(&mut band_dyn);
            audio.publish_slice(STREAM_IX.band_level, &band_dyn);
        }
        if n.is_multiple_of(2) {
            if want_pre {
                let bins = an_pre.compute(&mut spectrum);
                audio.publish_slice(STREAM_IX.spectrum_pre, &spectrum[..bins]);
            }
            if want_post {
                let bins = an_post.compute(&mut spectrum);
                audio.publish_slice(STREAM_IX.spectrum_post, &spectrum[..bins]);
            }
            if want_sc {
                let bins = an_sc.compute(&mut spectrum);
                audio.publish_slice(STREAM_IX.spectrum_sc, &spectrum[..bins]);
            }
        }
        if changed || n == 1 {
            engine.curve(&mut curve, CURVE_MIN_HZ, CURVE_MAX_HZ);
            audio.publish_slice(STREAM_IX.curve, &curve);
        }

        // 5. Pace to real time. If we fell far behind (debugger, laptop
        //    sleep) resync instead of rendering a burst of catch-up blocks.
        next += block_dur;
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        } else if now - next > Duration::from_millis(200) {
            next = now;
        }
    }
}

/// A starting point so the page is not empty: a 24 dB/oct low cut at 32 Hz,
/// a little body at 110 Hz, a dynamic dip in the mud around 320 Hz, a wide
/// high shelf for air, and a bell at 3.2 kHz on the side channel only so
/// the mid/side placement colour shows. Applied through the bridge, so the
/// page sees it like any other host value.
fn apply_demo_preset(s: &Vst3WebStratum) {
    let set = |id: &str, v: f32| {
        if let Some(i) = s.index_of(id) {
            s.set_param(i, v);
        }
    };
    set("b1_on", 1.0);
    set("b1_shape", 2.0);
    set("b1_freq", 32.0);
    set("b1_slope", 3.0);
    set("b2_on", 1.0);
    set("b2_shape", 0.0);
    set("b2_freq", 110.0);
    set("b2_gain", 2.5);
    set("b2_q", 1.2);
    set("b3_on", 1.0);
    set("b3_shape", 0.0);
    set("b3_freq", 320.0);
    set("b3_gain", -2.0);
    set("b3_q", 1.4);
    set("b3_dyn_on", 1.0);
    set("b3_dyn_range", -5.0);
    set("b4_on", 1.0);
    set("b4_shape", 3.0);
    set("b4_freq", 9000.0);
    set("b4_gain", 2.0);
    set("b4_q", 0.6);
    set("b5_on", 1.0);
    set("b5_shape", 0.0);
    set("b5_freq", 3200.0);
    set("b5_gain", 1.5);
    set("b5_q", 2.0);
    set("b5_place", 3.0);
}

/// Open `url` in the system browser (`start` / `open` / `xdg-open`).
fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        log::warn!("could not open browser: {e}");
    }
}

/// Parse the arguments, resolve the asset directory, build the bridge with
/// the demo preset, start the fake audio thread and the server, then run
/// the host loop forever (Ctrl+C ends the process; the discovery record is
/// then stale and cleaned up by the next `/instances` scan).
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut port: Option<u16> = None;
    let mut open = false;
    let mut dir: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" | "-p" => port = args.next().and_then(|v| v.parse().ok()),
            "--open" | "-o" => open = true,
            "--dir" | "-d" => dir = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("noob-q-standalone [--port N] [--open] [--dir path]");
                return;
            }
            other => log::warn!("ignoring argument {other}"),
        }
    }
    // Assets: `--dir`, else `web/dist` next to this crate's sources. `built`
    // only drives the hint printed below; the server serves whatever exists.
    let web = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/web"));
    let dist = web.join("dist");
    let (dir, built) = match dir {
        Some(d) => (d, true),
        None => (dist.clone(), dist.join("index.html").is_file()),
    };
    let dir = dir.canonicalize().unwrap_or(dir);

    // The bridge (parameters, streams, meta) is shared with the plug-in's
    // layout; the audio handle is the only thing the audio thread touches.
    let (bridge, params) = dsp::build_bridge("noob-q", SR);
    apply_demo_preset(&bridge);
    let audio = bridge.take_audio().expect("audio handle");
    let stats = Arc::new(Stats {
        blocks: AtomicU64::new(0),
        latency: AtomicUsize::new(0),
    });
    {
        let stats = stats.clone();
        thread::Builder::new()
            .name("fake-audio".into())
            .spawn(move || audio_thread(audio, params, stats))
            .expect("spawn audio thread");
    }

    // Presets, favourites and references the page keeps in `client.store`
    // persist in a file next to the discovery records (the plug-in keeps
    // them in its host state instead).
    let store = FileStore::attach(&bridge, FileStore::default_path("noob-q"));

    // `--port N` insists on that port; otherwise start at 4242 and walk up,
    // so a second copy (or another vst3-web-stratum app) does not collide.
    let cfg = match port {
        Some(p) => ServerConfig::default().port(p),
        None => ServerConfig::default().prefer_port(4242),
    };
    let server = vst3_web_stratum::serve(&bridge, cfg.assets_dir(&dir)).expect("start server");
    println!();
    println!("  noob-q standalone {}", server.url());
    println!("  websocket         {}", server.ws_url());
    println!("  assets            {}", dir.display());
    println!("  ui store          {}", store.path().display());
    println!("  instances         node tools/instances.mjs");
    if !built {
        println!();
        println!("  web/dist not found. Either build the SPA once:");
        println!("      cd examples/noob-q/web && npm install && npm run build");
        println!("  or develop with hot reload (proxies /ws to this server):");
        println!(
            "      cd examples/noob-q/web && VST3_WEB_STRATUM_PORT={} npm run dev",
            server.port()
        );
    }
    println!("  bench             node tools/bench.mjs {}", server.port());
    println!();
    if open {
        open_browser(&server.url());
    }

    // Host side: a real plug-in forwards edit gestures to the DAW here (the
    // nih-plug adapter does it from a UI-thread timer). The standalone just
    // counts them; the bridge has already applied the values. A 5 ms tick is
    // plenty for messages that arrive at human speed, and the store flush is
    // a no-op unless something changed.
    let mut last_status = Instant::now();
    let mut edits = 0u64;
    loop {
        bridge.drain_edits(|e| {
            edits += 1;
            log::debug!(
                "edit from client {}: #{} {:?} -> {:.4}",
                e.client,
                e.index,
                e.phase,
                e.value
            );
        });
        while let Some(m) = bridge.poll_message() {
            match m.topic.as_str() {
                "reset" => {
                    for i in 0..bridge.param_count() {
                        let d = bridge.spec(i).map(|s| s.default).unwrap_or(0.0);
                        bridge.set_param(i, d);
                    }
                    log::info!("client {} reset all parameters", m.client);
                }
                "resize" => {} // meaningful only inside a plugin window
                other => log::info!("message from client {}: {other} {}", m.client, m.data),
            }
        }
        if last_status.elapsed() >= Duration::from_secs(1) {
            last_status = Instant::now();
            let latency = stats.latency.load(Ordering::Relaxed);
            bridge.send_json(
                "status",
                json!({
                    "clients": server.client_count(),
                    "blocks": stats.blocks.load(Ordering::Relaxed),
                    "edits": edits,
                    "dropped": bridge.dropped_ui_changes(),
                    "sample_rate": SR,
                    "block": BLOCK,
                    "latency_samples": latency,
                    "latency_ms": latency as f32 * 1000.0 / SR,
                }),
            );
        }
        if let Err(e) = store.flush() {
            log::warn!("could not save the UI store: {e}");
        }
        thread::sleep(Duration::from_millis(5));
    }
}
