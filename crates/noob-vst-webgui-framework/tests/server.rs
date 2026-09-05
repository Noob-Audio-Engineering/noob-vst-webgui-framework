//! End-to-end tests: a real server on a real loopback socket, a real
//! WebSocket client (`tokio-tungstenite`), every frame kind.
//!
//! What each test proves:
//!
//! * `full_round_trip`: the connect sequence (Hello, manifest, snapshot); a
//!   client edit is applied to the store, queued for the host with its
//!   gesture phases and echoed back with the ECHO flag; a host change
//!   arrives with the HOST flag; a published stream frame arrives intact;
//!   two quick publishes never deliver an older frame after a newer one;
//!   Ping is answered with a Pong carrying the client's timestamp; a
//!   disabled subscription silences a stream; JSON messages flow both ways;
//!   the built-in client library is served under `/noob-vst-webgui-framework/` and path
//!   traversal is refused; shutdown closes the socket.
//! * `events_flow_both_ways`: an `Events` frame reaches the audio handle
//!   in order, and an event sent from the audio handle arrives as an
//!   `EventsOut` frame.
//! * `sticky_streams_are_replayed_to_late_clients`: a sticky stream
//!   published before anyone connected is replayed during the handshake,
//!   the manifest flags it, a non-sticky stream is not replayed, and the
//!   handshake ends with `store.all`.
//! * `port_probing_gives_each_instance_its_own_port`: two servers with the
//!   same probe policy take consecutive ports from the same base, a fixed
//!   port that is taken fails instead of moving, and different names hash
//!   to different bases.
//! * `store_round_trips_and_hydrates_late_clients`: the handshake carries
//!   the (empty) store; a client `store.set` is visible to the plug-in; a
//!   plug-in `store_set` reaches the client as `store.changed`; a late
//!   client is hydrated with everything; an oversized value is refused with
//!   `store.error` and not stored.
//! * `instance_endpoint_and_discovery_file`: `/instance` describes the
//!   server (name, port, pid); the discovery file exists while running and
//!   is gone after shutdown.
//! * `two_clients_see_each_other`: one client's edit reaches the other as a
//!   plain update while the originator gets it back flagged as an echo.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use noob_vst_webgui_framework::wire::{self, EditPhase, Frame, PARAM_FLAG_ECHO, PARAM_FLAG_HOST};
use noob_vst_webgui_framework::{
    NoobVstWebguiFramework, ParamSpec, ServerConfig, StreamKind, StreamSpec, serve,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn next_msg(ws: &mut Ws) -> Message {
    timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("timed out waiting for a message")
        .expect("stream ended")
        .expect("ws error")
}

async fn next_binary(ws: &mut Ws) -> Vec<u8> {
    loop {
        match next_msg(ws).await {
            Message::Binary(b) => return b.to_vec(),
            Message::Text(_) | Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected {other:?}"),
        }
    }
}

fn bridge() -> NoobVstWebguiFramework {
    NoobVstWebguiFramework::builder("itest")
        .param(
            ParamSpec::new("gain", "Gain")
                .range(-24.0, 24.0)
                .default(0.0)
                .unit("dB"),
        )
        .param(
            ParamSpec::new("freq", "Freq")
                .range(20.0, 20000.0)
                .log()
                .default(1000.0),
        )
        .stream(
            StreamSpec::new("meter", 4)
                .kind(StreamKind::Meter)
                .channels(2),
        )
        .stream(StreamSpec::new("spectrum", 1025).kind(StreamKind::Spectrum))
        .build()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_round_trip() {
    let s = bridge();
    let mut audio = s.take_audio().unwrap();
    let server = serve(&s, ServerConfig::default()).unwrap();
    assert_ne!(server.port(), 0);

    let (mut ws, _) = tokio_tungstenite::connect_async(server.ws_url())
        .await
        .expect("connect");

    // --- handshake: Hello, manifest, snapshot ------------------------------
    let hello = next_binary(&mut ws).await;
    let client_id = match Frame::decode(&hello).unwrap() {
        Frame::Hello {
            version,
            param_count,
            stream_count,
            client_id,
        } => {
            assert_eq!(version, wire::PROTOCOL_VERSION);
            assert_eq!(param_count, 2);
            assert_eq!(stream_count, 2);
            assert_ne!(client_id, 0);
            client_id
        }
        other => panic!("expected hello, got {other:?}"),
    };
    match next_msg(&mut ws).await {
        Message::Text(t) => {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap();
            assert_eq!(v["t"], "manifest");
            assert_eq!(v["params"][0]["id"], "gain");
            assert_eq!(v["streams"][1]["capacity"], 1025);
        }
        other => panic!("expected manifest, got {other:?}"),
    }
    let snap = next_binary(&mut ws).await;
    match Frame::decode(&snap).unwrap() {
        Frame::ParamValues(v) => {
            let e: Vec<_> = v.iter().collect();
            assert_eq!(e.len(), 2);
            assert!((e[0].value - 0.5).abs() < 1e-6);
        }
        other => panic!("expected snapshot, got {other:?}"),
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(server.client_count(), 1);

    // --- client edit: applied, queued for host, echoed --------------------
    let mut buf = Vec::new();
    let mut w = wire::ParamEditWriter::begin(&mut buf);
    w.push(0, EditPhase::Begin, 0.75)
        .push(0, EditPhase::Perform, 0.8);
    w.finish();
    ws.send(Message::Binary(buf.clone().into())).await.unwrap();

    let echo = next_binary(&mut ws).await;
    match Frame::decode(&echo).unwrap() {
        Frame::ParamValues(v) => {
            let e: Vec<_> = v.iter().collect();
            assert_eq!(e.len(), 2);
            assert_eq!(e[0].flags & PARAM_FLAG_ECHO, PARAM_FLAG_ECHO);
            assert!((e[1].value - 0.8).abs() < 1e-6);
        }
        other => panic!("expected echo, got {other:?}"),
    }
    assert!((s.param_norm(0) - 0.8).abs() < 1e-6);
    assert!((s.param(0) - 14.4).abs() < 1e-3);
    let mut edits = Vec::new();
    s.drain_edits(|e| edits.push(e));
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0].phase, EditPhase::Begin);
    assert_eq!(edits[1].phase, EditPhase::Perform);
    assert_eq!(edits[1].client, client_id);

    // --- host change reaches the client with the host flag ----------------
    s.set_param(1, 100.0);
    let hv = next_binary(&mut ws).await;
    match Frame::decode(&hv).unwrap() {
        Frame::ParamValues(v) => {
            let e: Vec<_> = v.iter().collect();
            assert_eq!(e[0].index, 1);
            assert_eq!(e[0].flags & PARAM_FLAG_HOST, PARAM_FLAG_HOST);
            assert!((e[0].value - 0.233).abs() < 1e-3);
        }
        other => panic!("expected host value, got {other:?}"),
    }

    // --- audio thread publishes a stream frame ----------------------------
    audio.publish_slice(0, &[0.5, 0.25, 0.1, 0.05]);
    let sf = next_binary(&mut ws).await;
    match Frame::decode(&sf).unwrap() {
        Frame::StreamF32 {
            stream, seq, data, ..
        } => {
            assert_eq!(stream, 0);
            assert_eq!(seq, 1);
            let v: Vec<f32> = wire::f32_iter(data).collect();
            assert_eq!(v, vec![0.5, 0.25, 0.1, 0.05]);
        }
        other => panic!("expected stream frame, got {other:?}"),
    }

    // --- two quick publishes: the newest always arrives, in order, and the
    //     pump never delivers a frame older than one already sent ----------
    let bins: Vec<f32> = (0..1025).map(|i| i as f32).collect();
    audio.publish_slice(1, &bins);
    audio.publish(1, |out| {
        for (i, o) in out.iter_mut().enumerate() {
            *o = -(i as f32);
        }
        1025
    });
    let mut last_seq = 0;
    let mut newest_seen = false;
    for _ in 0..2 {
        let sf = next_binary(&mut ws).await;
        match Frame::decode(&sf).unwrap() {
            Frame::StreamF32 {
                stream, seq, data, ..
            } => {
                assert_eq!(stream, 1);
                assert!(seq > last_seq, "frames must be monotonic");
                last_seq = seq;
                let mut out = vec![0f32; 1025];
                assert_eq!(wire::read_f32s(data, &mut out), 1025);
                if out[1] <= 0.0 {
                    newest_seen = true;
                    break;
                }
            }
            other => panic!("expected stream frame, got {other:?}"),
        }
    }
    assert!(newest_seen, "the newest frame never arrived");

    // --- ping / pong --------------------------------------------------------
    wire::encode_ping(&mut buf, 1234.5);
    ws.send(Message::Binary(buf.clone().into())).await.unwrap();
    let pong = next_binary(&mut ws).await;
    match Frame::decode(&pong).unwrap() {
        Frame::Pong { client_time, .. } => assert_eq!(client_time, 1234.5),
        other => panic!("expected pong, got {other:?}"),
    }

    // --- subscribe: disable stream 0, publish, nothing arrives ------------
    wire::encode_subscribe(&mut buf, 0, 0, false);
    ws.send(Message::Binary(buf.clone().into())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    audio.publish_slice(0, &[1.0, 1.0, 1.0, 1.0]);
    let quiet = timeout(Duration::from_millis(150), ws.next()).await;
    assert!(quiet.is_err(), "disabled stream still delivered: {quiet:?}");

    // --- JSON both ways -----------------------------------------------------
    s.send_json("preset", serde_json::json!({ "name": "Init" }));
    match next_msg(&mut ws).await {
        Message::Text(t) => {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap();
            assert_eq!(v["topic"], "preset");
            assert_eq!(v["data"]["name"], "Init");
        }
        other => panic!("expected json, got {other:?}"),
    }
    ws.send(Message::Text(
        r#"{"t":"msg","topic":"hello","data":{"x":1}}"#.into(),
    ))
    .await
    .unwrap();
    let mut got = None;
    for _ in 0..50 {
        if let Some(m) = s.poll_message() {
            got = Some(m);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let got = got.expect("inbound message");
    assert_eq!(got.topic, "hello");
    assert_eq!(got.data["x"], 1);
    assert_eq!(got.client, client_id);

    // --- static: built-in client library is served -------------------------
    let mut tcp = tokio::net::TcpStream::connect(server.addr()).await.unwrap();
    tcp.write_all(
        b"GET /noob-vst-webgui-framework/noob-vst-webgui-framework.js HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await
    .unwrap();
    let mut resp = String::new();
    tcp.read_to_string(&mut resp).await.unwrap();
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    assert!(resp.contains("text/javascript"));
    assert!(resp.contains("export class NoobVstWebguiFrameworkClient"));

    let mut tcp = tokio::net::TcpStream::connect(server.addr()).await.unwrap();
    tcp.write_all(b"GET /../etc/passwd HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut resp = String::new();
    tcp.read_to_string(&mut resp).await.unwrap();
    assert!(!resp.starts_with("HTTP/1.1 200"), "{resp}");

    // --- shutdown closes the socket ----------------------------------------
    server.shutdown();
    let end = timeout(Duration::from_secs(2), async {
        loop {
            match ws.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {}
            }
        }
    })
    .await;
    assert!(end.is_ok(), "socket stayed open after shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_flow_both_ways() {
    use noob_vst_webgui_framework::wire::{EventsWriter, UiEvent, event_kind};
    let s = bridge();
    let audio = s.take_audio().unwrap();
    let server = serve(&s, ServerConfig::default()).unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(server.ws_url())
        .await
        .unwrap();
    next_binary(&mut ws).await;
    next_msg(&mut ws).await;
    next_binary(&mut ws).await;

    // UI -> audio thread: a note on and off in one frame.
    let mut buf = Vec::new();
    let mut w = EventsWriter::begin(&mut buf, false);
    w.push(UiEvent::note_on(0, 64, 0.9))
        .push(UiEvent::note_off(0, 64, 0.0));
    w.finish();
    ws.send(Message::Binary(buf.clone().into())).await.unwrap();
    let mut got = Vec::new();
    for _ in 0..50 {
        audio.drain_events(|e| got.push(e));
        if got.len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].kind, event_kind::NOTE_ON);
    assert_eq!(got[0].a, 64);
    assert!((got[0].value - 0.9).abs() < 1e-6);
    assert_eq!(got[1].kind, event_kind::NOTE_OFF);

    // Audio thread -> UI: the plugin lights a key.
    assert!(audio.send_event(UiEvent::note_on(2, 48, 0.5)));
    let out = next_binary(&mut ws).await;
    match Frame::decode(&out).unwrap() {
        Frame::EventsOut(ev) => {
            let v: Vec<_> = ev.iter().collect();
            assert_eq!(v.len(), 1);
            assert_eq!(
                (v[0].kind, v[0].channel, v[0].a),
                (event_kind::NOTE_ON, 2, 48)
            );
        }
        other => panic!("expected events, got {other:?}"),
    }
    server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sticky_streams_are_replayed_to_late_clients() {
    let s = NoobVstWebguiFramework::builder("sticky")
        .param(ParamSpec::new("g", "G"))
        .stream(StreamSpec::new("curve", 4).kind(StreamKind::Curve).sticky())
        .stream(StreamSpec::new("meter", 2).kind(StreamKind::Meter))
        .build();
    let mut audio = s.take_audio().unwrap();
    let server = serve(&s, ServerConfig::default()).unwrap();
    // Published before anyone connects.
    audio.publish_slice(0, &[1.0, 2.0, 3.0, 4.0]);
    audio.publish_slice(1, &[0.5, 0.5]);
    tokio::time::sleep(Duration::from_millis(30)).await;

    let (mut ws, _) = tokio_tungstenite::connect_async(server.ws_url())
        .await
        .unwrap();
    next_binary(&mut ws).await; // hello
    match next_msg(&mut ws).await {
        Message::Text(t) => {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap();
            assert_eq!(v["streams"][0]["sticky"], true);
            assert_eq!(v["streams"][1]["sticky"], false);
        }
        other => panic!("expected manifest, got {other:?}"),
    }
    next_binary(&mut ws).await; // snapshot
    // The sticky curve arrives right away; the meter does not.
    let replay = next_binary(&mut ws).await;
    match Frame::decode(&replay).unwrap() {
        Frame::StreamF32 { stream, data, .. } => {
            assert_eq!(stream, 0);
            let v: Vec<f32> = wire::f32_iter(data).collect();
            assert_eq!(v, vec![1.0, 2.0, 3.0, 4.0]);
        }
        other => panic!("expected the sticky frame, got {other:?}"),
    }
    // Then the store hydration, then nothing: the meter is not replayed.
    match next_msg(&mut ws).await {
        Message::Text(t) => assert!(t.contains("store.all"), "{t}"),
        other => panic!("expected store.all, got {other:?}"),
    }
    let quiet = timeout(Duration::from_millis(100), ws.next()).await;
    assert!(quiet.is_err(), "non-sticky stream was replayed: {quiet:?}");
    server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn port_probing_gives_each_instance_its_own_port() {
    use noob_vst_webgui_framework::PortPolicy;
    let policy = PortPolicy::for_name("probe-test");
    let PortPolicy::Probe { base, .. } = policy else {
        panic!("expected probe")
    };
    let a = serve(
        &bridge(),
        ServerConfig::default().port_policy(policy).discovery(false),
    )
    .unwrap();
    let b = serve(
        &bridge(),
        ServerConfig::default().port_policy(policy).discovery(false),
    )
    .unwrap();
    // Same policy, two instances: consecutive ports from the same base.
    assert!(a.port() >= base && a.port() < base + 64, "{}", a.port());
    assert_eq!(b.port(), a.port() + 1);
    // A fixed port that is taken fails loudly instead of silently moving.
    let taken = serve(
        &bridge(),
        ServerConfig::default().port(a.port()).discovery(false),
    );
    assert!(taken.is_err());
    // Different names land on different bases.
    assert_ne!(
        PortPolicy::for_name("noob-q"),
        PortPolicy::for_name("noob-wave")
    );
    a.shutdown();
    b.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn store_round_trips_and_hydrates_late_clients() {
    let s = bridge();
    let server = serve(&s, ServerConfig::default().discovery(false)).unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(server.ws_url())
        .await
        .unwrap();
    next_binary(&mut ws).await; // hello
    next_msg(&mut ws).await; // manifest
    next_binary(&mut ws).await; // snapshot
    // Handshake ends with the (empty) store.
    match next_msg(&mut ws).await {
        Message::Text(t) => {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap();
            assert_eq!(v["topic"], "store.all");
            assert!(v["data"]["values"].as_object().unwrap().is_empty());
        }
        other => panic!("expected store.all, got {other:?}"),
    }
    // Client writes a key; the plugin side sees it.
    ws.send(Message::Text(
        r#"{"t":"msg","topic":"store.set","data":{"key":"presets","value":[{"name":"A"}]}}"#.into(),
    ))
    .await
    .unwrap();
    let mut got = None;
    for _ in 0..50 {
        if let Some(v) = s.store_get("presets") {
            got = Some(v);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(got.unwrap()[0]["name"], "A");
    // Plugin writes a key; the client hears about it.
    s.store_set("size", serde_json::json!("Large")).unwrap();
    match next_msg(&mut ws).await {
        Message::Text(t) => {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap();
            assert_eq!(v["topic"], "store.changed");
            assert_eq!(v["data"]["key"], "size");
            assert_eq!(v["data"]["value"], "Large");
        }
        other => panic!("expected store.changed, got {other:?}"),
    }
    // A late client gets everything at connect; JSON survives a round trip.
    let json = s.store_json();
    assert!(json.contains("\"size\":\"Large\""));
    let (mut late, _) = tokio_tungstenite::connect_async(server.ws_url())
        .await
        .unwrap();
    next_binary(&mut late).await;
    next_msg(&mut late).await;
    next_binary(&mut late).await;
    match next_msg(&mut late).await {
        Message::Text(t) => {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap();
            assert_eq!(v["topic"], "store.all");
            assert_eq!(v["data"]["values"]["size"], "Large");
            assert_eq!(v["data"]["values"]["presets"][0]["name"], "A");
        }
        other => panic!("expected store.all, got {other:?}"),
    }
    // Oversized values are refused with an error, not stored.
    let big = "x".repeat(300 * 1024);
    ws.send(Message::Text(
        format!(r#"{{"t":"msg","topic":"store.set","data":{{"key":"big","value":"{big}"}}}}"#)
            .into(),
    ))
    .await
    .unwrap();
    match next_msg(&mut ws).await {
        Message::Text(t) => {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap();
            assert_eq!(v["topic"], "store.error");
        }
        other => panic!("expected store.error, got {other:?}"),
    }
    assert!(s.store_get("big").is_none());
    server.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn instance_endpoint_and_discovery_file() {
    use noob_vst_webgui_framework::discovery;
    let s = bridge();
    let server = serve(&s, ServerConfig::default()).unwrap();
    // /instance describes this server.
    let live = tokio::task::spawn_blocking({
        let port = server.port();
        move || discovery::probe(port, Duration::from_millis(500))
    })
    .await
    .unwrap()
    .expect("instance answers");
    assert_eq!(live.name, "itest");
    assert_eq!(live.port, server.port());
    assert_eq!(live.pid, std::process::id());
    // While running, the discovery file exists and lists us; after
    // shutdown it is gone.
    if discovery::dir().is_some() {
        let files = discovery::list_files();
        assert!(files.iter().any(|(_, i)| i.port == server.port()));
        let port = server.port();
        server.shutdown();
        let files = discovery::list_files();
        assert!(!files.iter().any(|(_, i)| i.port == port));
    } else {
        server.shutdown();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_clients_see_each_other() {
    let s = bridge();
    let server = serve(&s, ServerConfig::default()).unwrap();
    let (mut a, _) = tokio_tungstenite::connect_async(server.ws_url())
        .await
        .unwrap();
    let (mut b, _) = tokio_tungstenite::connect_async(server.ws_url())
        .await
        .unwrap();
    for ws in [&mut a, &mut b] {
        next_binary(ws).await; // hello
        next_msg(ws).await; // manifest
        next_binary(ws).await; // snapshot
    }
    let mut buf = Vec::new();
    let mut w = wire::ParamEditWriter::begin(&mut buf);
    w.push(1, EditPhase::Perform, 0.25);
    w.finish();
    a.send(Message::Binary(buf.into())).await.unwrap();

    let ea = next_binary(&mut a).await;
    let eb = next_binary(&mut b).await;
    let fa = match Frame::decode(&ea).unwrap() {
        Frame::ParamValues(v) => v.iter().next().unwrap(),
        other => panic!("{other:?}"),
    };
    let fb = match Frame::decode(&eb).unwrap() {
        Frame::ParamValues(v) => v.iter().next().unwrap(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        fa.flags & PARAM_FLAG_ECHO,
        PARAM_FLAG_ECHO,
        "originator gets echo flag"
    );
    assert_eq!(
        fb.flags & PARAM_FLAG_ECHO,
        0,
        "other client gets a plain update"
    );
    assert_eq!(fb.index, 1);
    assert!((fb.value - 0.25).abs() < 1e-6);
    server.shutdown();
}

/// Blocking HTTP GET against the local server (mirrors `discovery::probe`).
fn http_get(port: u16, path: &str) -> String {
    use std::io::{Read, Write};
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
    let mut s = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    s.write_all(
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .unwrap();
    let mut buf = Vec::new();
    // Read to EOF, and do **not** discard the error. `read_to_end` returns
    // `Err` with whatever it already had when the timeout trips, so ignoring
    // it silently yields a truncated body, which then fails to parse as JSON
    // somewhere far away and looks like a server fault rather than a short
    // read. Under a parallel test run that is exactly what happened.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let mut chunk = [0u8; 8192];
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                assert!(
                    std::time::Instant::now() < deadline,
                    "GET {path} timed out after {} bytes",
                    buf.len()
                );
            }
            Err(e) => panic!("GET {path} failed after {} bytes: {e}", buf.len()),
        }
    }
    let text = String::from_utf8_lossy(&buf);
    text.split("\r\n\r\n")
        .nth(1)
        .unwrap_or_else(|| panic!("GET {path}: no body in {} bytes", buf.len()))
        .trim()
        .to_string()
}

/// `/instances` lists only instances of the same plug-in unless `?all=1`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn instances_are_scoped_to_the_plugin_name() {
    use noob_vst_webgui_framework::discovery;
    if discovery::dir().is_none() {
        return;
    }
    let a = serve(&bridge(), ServerConfig::default()).unwrap(); // "itest"
    let other = NoobVstWebguiFramework::builder("itest-other")
        .param(ParamSpec::new("gain", "Gain"))
        .build();
    let b = serve(&other, ServerConfig::default()).unwrap();
    let (pa, pb) = (a.port(), b.port());
    let same: Vec<discovery::Instance> = tokio::task::spawn_blocking(move || {
        serde_json::from_str(&http_get(pa, "/instances")).unwrap()
    })
    .await
    .unwrap();
    assert!(same.iter().any(|i| i.port == pa), "lists itself");
    assert!(
        same.iter().all(|i| i.name == "itest"),
        "only the same plug-in"
    );
    assert!(
        !same.iter().any(|i| i.port == pb),
        "the other plug-in is left out"
    );
    let all: Vec<discovery::Instance> = tokio::task::spawn_blocking(move || {
        serde_json::from_str(&http_get(pa, "/instances?all=1")).unwrap()
    })
    .await
    .unwrap();
    assert!(
        all.iter().any(|i| i.port == pb),
        "?all=1 lists every instance"
    );
    b.shutdown();
    a.shutdown();
}
