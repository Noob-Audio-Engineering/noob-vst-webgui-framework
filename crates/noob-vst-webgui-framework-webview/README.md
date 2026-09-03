# noob-vst-webgui-framework-webview

Embed the operating system's own web view inside a window a plug-in host
hands you, plus a UI-thread timer for adapters that need one. Part of
[noob-vst-webgui-framework](../../README.md).

Nothing is bundled. The page is rendered by the web engine the OS already
ships (WebView2 on Windows, WKWebView on macOS), so the plug-in binary stays
small and gets engine updates for free. The crate does not know anything
about audio or the noob-vst-webgui-framework protocol; it only knows how to put a URL inside a
parent window and how to get called back periodically on the host's UI
thread.

## Platform support

| platform | engine    | how                                   | status |
|----------|-----------|---------------------------------------|--------|
| Windows  | WebView2  | `wry` child of the host `HWND`        | supported, exercised by the plug-ins |
| macOS    | WKWebView | `wry` child of the host `NSView`      | supported by the code path; I have not exercised it yet |
| Linux    | WebKitGTK | needs a GTK parent; a raw X11 id is not enough | `Error::Unsupported`; adapters open the page in the system browser |

## Requirements

- **Windows**: the WebView2 Runtime. It ships with Windows 11 and with
  Microsoft Edge on Windows 10. Without it `EmbeddedWebView::new` returns
  `Error::Wry`, and the nih-plug adapter opens the page in the default
  browser instead.
- **macOS**: WKWebView is part of the OS; nothing to install.
- **Linux**: `wry` is built with its `os-webview` feature, so the
  `webkit2gtk-4.1` and `libsoup-3.0` development packages must be present at
  build time even though the child view is not created there yet.

Rust edition 2024; dependencies are `wry` (child web views), `raw-window-handle`
(the parent handle) and, on Windows, `windows-sys` (the timer).

## Quick start

```rust
use std::time::Duration;
use noob_vst_webgui_framework_webview::{EmbeddedWebView, RawParent, UiTimer, WebViewOptions};

// On the host's UI thread, with the HWND / NSView the host gave you:
let parent = RawParent::win32(hwnd).expect("non-null HWND");
let mut opts = WebViewOptions::new("http://127.0.0.1:4242/", 1000, 640);
opts.devtools = true;                 // right-click > Inspect
let view = EmbeddedWebView::new(&parent, opts)?;

// The host resized the editor:
view.resize(1200, 800)?;

// Periodic work on this thread, driven by the host's message loop
// (Windows; `None` elsewhere, then poll from another thread):
let timer = UiTimer::new(Duration::from_millis(3), || {
    // drain queued parameter edits and hand them to the host ...
});

// Keep `view` and `timer` alive while the window is open and drop them on
// this same thread when the host closes the editor.
```

Everything in the crate is bound to the host's UI thread: create, use and
drop the web view and the timer there. Sizes are logical pixels; the engine
applies the monitor's scale factor itself.

## The UI-thread timer

VST3 and CLAP want parameter edits delivered from the main thread, but a
plug-in does not own the host's message loop. `UiTimer` asks that loop to
call back: on Windows it is a `SetTimer` thread timer, whose `WM_TIMER`
message the host's pump dispatches to a callback kept in a thread-local
table (the callback is taken out of the table while it runs, so a re-entrant
message loop cannot borrow it twice). On other platforms `UiTimer::new`
returns `None` and the caller forwards from its network thread, which hosts
tolerate in practice.

## Where it is used

- [`noob-vst-webgui-framework-nih`](../noob-vst-webgui-framework-nih/README.md) creates the web view in
  `Editor::spawn` and uses the timer to forward edits.
- The Noob Audio Engineering plug-ins get it through that adapter.

## Further reading

- [Getting started](../../docs/GETTING-STARTED.md): building a plug-in with
  noob-vst-webgui-framework end to end.
- [Architecture](../../docs/ARCHITECTURE.md): threads, data flow and the
  real-time guarantees the rest of the stack makes.
- API docs: `cargo doc --no-deps -p noob-vst-webgui-framework-webview --open`.
