//! Embed the operating system's own web view inside a window the host hands
//! to a plugin editor, plus a UI-thread timer for adapters that need one.
//!
//! # What this crate is for
//!
//! A vst3-web-stratum plug-in serves its interface as a web page from a local server
//! (`vst3_web_stratum::serve`). This crate puts that page *inside* the editor
//! window the host provides, using the web engine the operating system
//! already ships. Nothing is bundled: the plug-in binary stays small, no
//! browser engine has to be built or licensed, and the page gets engine
//! updates from the OS. The nih-plug adapter (`vst3-web-stratum-nih`) is the main
//! consumer; any other plug-in framework that can hand over a raw parent
//! window can use it the same way.
//!
//! The crate is deliberately thin:
//! * [`RawParent`] wraps the parent window handle the host gives you.
//! * [`WebViewOptions`] says what to load and how big.
//! * [`EmbeddedWebView`] is the child web view itself.
//! * [`Error`] tells you why it could not be created.
//! * [`UiTimer`] is a periodic callback on the host's UI thread.
//!
//! Everything else (protocol, parameters, streams, the server) lives in
//! `vst3-web-stratum`.
//!
//! # Platform support
//!
//! | platform | engine    | how                                                   | status |
//! |----------|-----------|-------------------------------------------------------|--------|
//! | Windows  | WebView2  | `wry` child of the host `HWND`                        | supported, exercised by the examples |
//! | macOS    | WKWebView | `wry` child of the host `NSView`                      | supported by the code path; not yet exercised by me           |
//! | Linux    | WebKitGTK | needs a GTK parent; a raw X11 window id is not enough | [`Error::Unsupported`]; adapters fall back to the system browser |
//!
//! Requirements:
//! * **Windows**: the WebView2 Runtime. It ships with Windows 11 and with
//!   Microsoft Edge on Windows 10. On a machine without it
//!   [`EmbeddedWebView::new`] returns [`Error::Wry`] and the adapter opens
//!   the page in the default browser instead.
//! * **macOS**: WKWebView is part of the OS; nothing to install.
//! * **Linux**: `wry` is built with its `os-webview` feature, so the
//!   `webkit2gtk-4.1` and `libsoup-3.0` development packages are needed at
//!   build time even though the child view is not created there yet.
//!
//! When a parent cannot host a web view, the adapter is expected to open the
//! page URL in the system browser. The plug-in keeps working (the page talks
//! to the same local server); only the window is somewhere else.
//!
//! # Threading
//!
//! Every type here is bound to the host's UI (GUI) thread:
//! * [`EmbeddedWebView`] must be created, used and dropped on the thread that
//!   owns the parent window. It is neither `Send` nor `Sync`.
//! * [`UiTimer`] fires on the thread that created it, from that thread's own
//!   message loop, and must be dropped on that thread too.
//!
//! The network and audio threads never touch this crate; they reach the page
//! through `vst3-web-stratum`.
//!
//! # The UI-thread timer
//!
//! VST3 and CLAP want parameter edits (begin / perform / end) to reach the
//! host from the main (GUI) thread. Edits arrive on vst3-web-stratum's network thread,
//! so an adapter queues them and needs something *on the GUI thread* to drain
//! the queue. A plug-in does not own the host's message loop and cannot run
//! its own, so [`UiTimer`] asks the host's loop to call back periodically:
//!
//! * On Windows it is a `SetTimer` with a null window handle, which makes it
//!   a *thread* timer: the `WM_TIMER` message is posted to the queue of the
//!   creating thread, and the host's message pump dispatches it to our
//!   callback. Callbacks live in a thread-local table keyed by timer id. A
//!   callback is taken out of the table while it runs, so a re-entrant
//!   message loop (a modal dialog opened from inside the callback, a nested
//!   `WM_TIMER` during a long callback) cannot borrow the table twice.
//! * On other platforms [`UiTimer::new`] returns `None`. Adapters then
//!   forward edits directly from the network thread, which every major host
//!   tolerates in practice. A `CFRunLoopTimer` for macOS is the natural next
//!   step.
//!
//! # Sizes and DPI
//!
//! Widths and heights are in *logical* pixels; the engine applies the
//! monitor's scale factor itself, so a page laid out for 1000×640 looks the
//! same at 100 % and 150 % scaling. Hosts that report a scale factor can be
//! told to ignore it (the nih-plug adapter answers `false` to
//! `set_scale_factor`).
//!
//! # Example
//!
//! ```ignore
//! use vst3_web_stratum_webview::{EmbeddedWebView, RawParent, UiTimer, WebViewOptions};
//!
//! // On the host's UI thread, with the parent HWND / NSView the host gave you:
//! let parent = RawParent::win32(hwnd).expect("non-null HWND");
//! let mut opts = WebViewOptions::new("http://127.0.0.1:4242/", 1000, 640);
//! opts.devtools = true;
//! let view = EmbeddedWebView::new(&parent, opts)?;
//!
//! // Later, when the host resizes the window:
//! view.resize(1200, 800)?;
//!
//! // Do periodic work on this thread (Windows; `None` elsewhere):
//! let timer = UiTimer::new(std::time::Duration::from_millis(3), || {
//!     // drain queued edits, forward them to the host ...
//! });
//! // Keep `view` and `timer` alive as long as the window is open; drop them
//! // on this same thread when the host closes the editor.
//! ```

use std::fmt;
use std::num::NonZeroIsize;
use std::ptr::NonNull;
use std::time::Duration;

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};

/// A parent window handed over by the host, as a raw handle.
///
/// Build one with [`win32`](Self::win32), [`appkit`](Self::appkit) or
/// [`x11`](Self::x11) from whatever the plug-in framework gives you, then
/// pass it to [`EmbeddedWebView::new`]. The wrapped
/// [`RawWindowHandle`] is public so other `raw-window-handle` consumers can
/// reuse it.
///
/// The handle is *borrowed*: the host owns the window and keeps it alive for
/// as long as the editor is open. Do not keep a `RawParent` past the point
/// where the host closes the editor.
#[derive(Clone, Copy, Debug)]
pub struct RawParent(pub RawWindowHandle);

impl RawParent {
    /// Windows: an `HWND`.
    ///
    /// Returns `None` for a null handle, which is what a host would pass if
    /// it had no window to offer.
    pub fn win32(hwnd: *mut std::ffi::c_void) -> Option<Self> {
        let h = raw_window_handle::Win32WindowHandle::new(NonZeroIsize::new(hwnd as isize)?);
        Some(RawParent(RawWindowHandle::Win32(h)))
    }
    /// macOS: an `NSView*` (VST3 passes the content view, CLAP the "cocoa"
    /// view; both are `NSView`s).
    ///
    /// Returns `None` for a null pointer.
    pub fn appkit(ns_view: *mut std::ffi::c_void) -> Option<Self> {
        let h = raw_window_handle::AppKitWindowHandle::new(NonNull::new(ns_view)?);
        Some(RawParent(RawWindowHandle::AppKit(h)))
    }
    /// Linux: an X11 window id.
    ///
    /// Always succeeds, but [`EmbeddedWebView::new`] currently answers
    /// [`Error::Unsupported`] for it: WebKitGTK needs a GTK parent, and the
    /// id alone is not one. It exists so adapters can build the handle
    /// uniformly and decide on the fallback in one place.
    // `c_ulong` is `u32` on Windows and `u64` elsewhere; the conversion is
    // real on Linux and an identity on Windows, where clippy would call it useless.
    #[allow(clippy::useless_conversion)]
    pub fn x11(window: u32) -> Self {
        RawParent(RawWindowHandle::Xlib(
            raw_window_handle::XlibWindowHandle::new(std::ffi::c_ulong::from(window)),
        ))
    }
}

impl HasWindowHandle for RawParent {
    /// Lends the raw handle to `wry` (and anything else that speaks
    /// `raw-window-handle`) without taking ownership of the window.
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: the handle came from the host and outlives the editor.
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}

/// Why a web view could not be created or driven.
///
/// Both variants are recoverable from the plug-in's point of view: log them
/// and open the page URL in the system browser instead.
#[derive(Debug)]
pub enum Error {
    /// This platform / parent-handle combination is not supported (today:
    /// anything that is not a Win32 `HWND` or an AppKit `NSView`). The
    /// payload says which combination was refused.
    Unsupported(&'static str),
    /// The engine refused: no WebView2 runtime installed, the parent window
    /// is gone, an invalid URL, or a script error from
    /// [`EmbeddedWebView::eval`].
    Wry(wry::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported(why) => write!(f, "web view unsupported here: {why}"),
            Error::Wry(e) => write!(f, "web view: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<wry::Error> for Error {
    fn from(e: wry::Error) -> Self {
        Error::Wry(e)
    }
}

/// What to load and how to present it. Build with [`new`](Self::new) and
/// adjust the public fields before handing it to [`EmbeddedWebView::new`].
pub struct WebViewOptions {
    /// The page to load; for a vst3-web-stratum plug-in the URL of its own server,
    /// e.g. `http://127.0.0.1:4242/`.
    pub url: String,
    /// Initial width in logical pixels (see the crate docs on DPI).
    pub width: u32,
    /// Initial height in logical pixels.
    pub height: u32,
    /// Enable the engine's developer tools (right-click > Inspect, or
    /// [`EmbeddedWebView::open_devtools`]). Defaults to `true` in debug
    /// builds and `false` in release builds.
    pub devtools: bool,
    /// RGBA background painted before the page loads, so the window does not
    /// flash white while the page's own stylesheet arrives. Defaults to the
    /// examples' dark ink colour, `(13, 16, 22, 255)`.
    pub background: (u8, u8, u8, u8),
    /// JavaScript run in every page before its own scripts (for example to
    /// define `window.vst3WebStratumHost = {...}` so the page can tell it is
    /// embedded). `None` by default.
    pub init_script: Option<String>,
}

impl WebViewOptions {
    /// Options for `url` at `width` × `height` logical pixels, with
    /// `devtools` on in debug builds, a dark background and no init script.
    pub fn new(url: impl Into<String>, width: u32, height: u32) -> Self {
        WebViewOptions {
            url: url.into(),
            width,
            height,
            devtools: cfg!(debug_assertions),
            background: (13, 16, 22, 255),
            init_script: None,
        }
    }
}

/// A web view living inside the host's window.
///
/// Must be created, used and dropped on the host's UI thread; the type is
/// intentionally neither `Send` nor `Sync`. Dropping it destroys the child
/// window. The view fills the parent from its top-left corner; call
/// [`resize`](Self::resize) whenever the host changes the editor size.
pub struct EmbeddedWebView {
    inner: wry::WebView,
}

impl EmbeddedWebView {
    /// Create the child web view inside `parent` and start loading
    /// `opts.url`.
    ///
    /// Call on the UI thread that owns `parent`. The page starts loading
    /// asynchronously; this returns as soon as the engine has been set up.
    ///
    /// # Errors
    ///
    /// * [`Error::Unsupported`] when `parent` is not a Win32 or AppKit
    ///   handle (Linux / X11 today).
    /// * [`Error::Wry`] when the engine cannot be created: no WebView2
    ///   runtime on Windows, a parent that is not a valid window, or an
    ///   engine-specific failure. The message from `wry` is preserved.
    pub fn new(parent: &RawParent, opts: WebViewOptions) -> Result<Self, Error> {
        match parent.0 {
            RawWindowHandle::Win32(_) | RawWindowHandle::AppKit(_) => {}
            _ => {
                return Err(Error::Unsupported(
                    "only Win32 and AppKit parents can host a child web view",
                ));
            }
        }
        let mut b = wry::WebViewBuilder::new()
            .with_url(&opts.url)
            .with_devtools(opts.devtools)
            .with_background_color(opts.background)
            .with_bounds(bounds(opts.width, opts.height));
        if let Some(js) = &opts.init_script {
            b = b.with_initialization_script(js);
        }
        let inner = b.build_as_child(parent)?;
        Ok(EmbeddedWebView { inner })
    }

    /// Resize the view to `width` × `height` logical pixels, keeping it at
    /// the parent's top-left corner. Call after the host has resized the
    /// editor window (or agreed to a resize request).
    ///
    /// # Errors
    ///
    /// [`Error::Wry`] if the engine rejects the new bounds.
    pub fn resize(&self, width: u32, height: u32) -> Result<(), Error> {
        self.inner.set_bounds(bounds(width, height))?;
        Ok(())
    }

    /// Load a different URL (for example after the server restarted on a
    /// new port).
    ///
    /// # Errors
    ///
    /// [`Error::Wry`] for a URL the engine refuses.
    pub fn navigate(&self, url: &str) -> Result<(), Error> {
        self.inner.load_url(url)?;
        Ok(())
    }

    /// Run JavaScript in the page. Fire-and-forget: there is no return value
    /// and errors inside the script surface in the page's console, not
    /// here.
    ///
    /// # Errors
    ///
    /// [`Error::Wry`] if the engine could not schedule the script (for
    /// example before the first page has been created).
    pub fn eval(&self, js: &str) -> Result<(), Error> {
        self.inner.evaluate_script(js)?;
        Ok(())
    }

    /// Open the engine's developer tools window. Does nothing when
    /// [`WebViewOptions::devtools`] was `false`, and does nothing in a
    /// release build unless this crate's `devtools` feature is enabled
    /// (wry only compiles the call under `debug_assertions` or its own
    /// `devtools` feature).
    pub fn open_devtools(&self) {
        #[cfg(any(debug_assertions, feature = "devtools"))]
        self.inner.open_devtools();
    }

    /// The underlying `wry` web view, for anything this wrapper does not
    /// expose (custom protocols, IPC handlers, zoom, ...).
    pub fn inner(&self) -> &wry::WebView {
        &self.inner
    }
}

/// Bounds for a view that fills the parent from its top-left corner, in
/// logical pixels.
fn bounds(width: u32, height: u32) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
        size: wry::dpi::LogicalSize::new(width as f64, height as f64).into(),
    }
}

// ---------------------------------------------------------------------------
// UI-thread timer
// ---------------------------------------------------------------------------

/// A periodic callback on the thread that created it, dispatched by that
/// thread's native message loop. Dropping it stops the timer.
///
/// See the crate docs for why plug-in adapters need this and how it is
/// implemented. In short: on Windows it is a `WM_TIMER` thread timer whose
/// callback lives in a thread-local table; elsewhere [`new`](Self::new)
/// returns `None` and callers do their periodic work from another thread.
///
/// The callback runs on the creating thread only, so it may touch
/// thread-bound objects such as an [`EmbeddedWebView`]. It should be quick
/// (a few microseconds of queue draining); the host's message loop is
/// blocked while it runs. Create and drop the timer on the same thread.
pub struct UiTimer {
    #[cfg(windows)]
    id: usize,
    #[cfg(not(windows))]
    _private: (),
}

/// Win32 implementation: `SetTimer` with a null `HWND` gives a thread timer
/// delivered through the creating thread's message queue.
#[cfg(windows)]
mod win_timer {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

    thread_local! {
        /// Callbacks of the timers created on this thread, keyed by the id
        /// `SetTimer` returned. Thread-local because `WM_TIMER` is only ever
        /// dispatched on the creating thread.
        static CALLBACKS: RefCell<HashMap<usize, Box<dyn FnMut()>>> = RefCell::new(HashMap::new());
    }

    /// The `TIMERPROC` the message loop calls for every `WM_TIMER` we own.
    unsafe extern "system" fn timer_proc(_hwnd: HWND, _msg: u32, id: usize, _time: u32) {
        // Take the callback out while it runs so a re-entrant message loop
        // cannot borrow the map twice.
        let cb = CALLBACKS.with(|c| c.borrow_mut().remove(&id));
        if let Some(mut cb) = cb {
            cb();
            CALLBACKS.with(|c| {
                c.borrow_mut().entry(id).or_insert(cb);
            });
        }
    }

    /// Start a thread timer firing every `interval_ms` (at least 1 ms; the
    /// OS rounds to its own timer resolution, typically 10–16 ms unless the
    /// host raised it). Returns the timer id, or `None` if `SetTimer`
    /// failed.
    pub fn start(interval_ms: u32, f: Box<dyn FnMut()>) -> Option<usize> {
        // SAFETY: plain Win32 call; a null HWND makes the timer thread-local.
        let id = unsafe {
            SetTimer(
                std::ptr::null_mut(),
                0,
                interval_ms.max(1),
                Some(timer_proc),
            )
        };
        if id == 0 {
            return None;
        }
        CALLBACKS.with(|c| c.borrow_mut().insert(id, f));
        Some(id)
    }

    /// Stop the timer and forget its callback. Must run on the thread that
    /// called [`start`].
    pub fn stop(id: usize) {
        // SAFETY: id came from SetTimer on this thread.
        unsafe {
            KillTimer(std::ptr::null_mut(), id);
        }
        CALLBACKS.with(|c| c.borrow_mut().remove(&id));
    }
}

impl UiTimer {
    /// Start a timer on the current thread that calls `f` about every
    /// `interval` (rounded up to whole milliseconds, minimum 1 ms; the real
    /// period is bounded below by the OS timer resolution).
    ///
    /// Returns `None` where no native timer is available (every platform
    /// except Windows today, or if the OS refused to create one); callers
    /// should then do the periodic work from another thread. The callback
    /// must be `'static` because the host's message loop, not this function,
    /// invokes it.
    #[allow(unused_variables)]
    pub fn new(interval: Duration, f: impl FnMut() + 'static) -> Option<UiTimer> {
        #[cfg(windows)]
        {
            let id = win_timer::start(interval.as_millis().max(1) as u32, Box::new(f))?;
            Some(UiTimer { id })
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
}

impl Drop for UiTimer {
    /// Stops the timer. Must happen on the creating thread.
    fn drop(&mut self) {
        #[cfg(windows)]
        win_timer::stop(self.id);
    }
}

/// The usable size of the monitor a host window is on, in logical pixels
/// (the work area: the screen minus the taskbar), so a page can declare
/// fullscreen intent and the plug-in can ask the host for exactly that size.
///
/// Windows only for now (`MonitorFromWindow` + `GetMonitorInfoW` on the
/// parent window); `None` where the platform or the handle does not allow
/// it, in which case callers fall back to the size the page reports from
/// `screen.availWidth` / `screen.availHeight`.
pub fn monitor_work_area(parent: &RawParent) -> Option<(u32, u32)> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        };
        let RawWindowHandle::Win32(h) = parent.0 else {
            return None;
        };
        // SAFETY: plain Win32 queries on a window handle the host gave us;
        // MONITORINFO is a POD struct whose size field we initialise.
        unsafe {
            let hwnd = h.hwnd.get() as *mut core::ffi::c_void;
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            if monitor.is_null() {
                return None;
            }
            let mut info: MONITORINFO = core::mem::zeroed();
            info.cbSize = core::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return None;
            }
            let r = info.rcWork;
            let (w, h) = (
                (r.right - r.left).max(0) as u32,
                (r.bottom - r.top).max(0) as u32,
            );
            if w == 0 || h == 0 { None } else { Some((w, h)) }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = parent;
        None
    }
}
