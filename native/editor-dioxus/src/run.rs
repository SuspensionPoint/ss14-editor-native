//! Starts Dioxus Native and times its frames.
//!
//! This is `dioxus_native::launch_cfg` written out by hand, for two things it doesn't offer. One
//! is the time each frame takes, measured around the window's redraw. The other is a window close
//! that can be held back while there are unsaved changes. Hot reloading and the HTML parser are
//! left out.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use blitz_traits::net::NetProvider;
use dioxus_native::prelude::*;
use dioxus_native::winit::application::ApplicationHandler;
use dioxus_native::winit::event::{StartCause, WindowEvent};
use dioxus_native::winit::event_loop::ActiveEventLoop;
use dioxus_native::winit::window::WindowId;
use dioxus_native::{DioxusDocument, DioxusNativeApplication, DioxusNativeWindowRenderer, DocumentConfig, RendererOptions};

use crate::dialogs::Discarding;

/// How many frames the performance display averages over.
const FRAMES: usize = 30;

/// What the last frames cost: the seconds each redraw took, from the window's asking to draw
/// to the picture handed to the screen, and when each began.
#[derive(Default)]
pub(crate) struct FrameTimes {
    pub(crate) costs: VecDeque<f64>,
    pub(crate) starts: VecDeque<Instant>,
    pub(crate) drawn: u64,
}

impl FrameTimes {
    /// The average cost of a frame in milliseconds, and frames a second, over the last frames.
    pub(crate) fn summary(&self) -> (Option<f64>, Option<f64>) {
        let cost = (!self.costs.is_empty()).then(|| 1000.0 * self.costs.iter().sum::<f64>() / self.costs.len() as f64);
        let rate = match (self.starts.front(), self.starts.back()) {
            (Some(first), Some(last)) if self.starts.len() > 1 && last > first => Some((self.starts.len() - 1) as f64 / (*last - *first).as_secs_f64()),
            _ => None,
        };
        (cost, rate)
    }
}

thread_local! {
    pub(crate) static FRAME_TIMES: RefCell<FrameTimes> = RefCell::new(FrameTimes::default());
    /// The document, for laying it out again before the mouse is tested against it.
    pub(crate) static DOCUMENT: RefCell<Option<dioxus_native::NodeHandle>> = const { RefCell::new(None) };
    /// Set when a component drew, which changes the document, and cleared once it is laid out.
    pub(crate) static STALE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Lays the document out again if a component drew since it last was.
///
/// Blitz finds what the mouse is over from the last layout, which still names the elements a
/// component took away since. The next mouse event that hits where one of them was stops the
/// program (blitz-dom 0.3.0-beta.4, `Node::hit_inner`). So the document is laid out before
/// each mouse event that follows a change, as the next frame would lay it out.
fn settle(started: Instant) {
    if !STALE.with(|stale| stale.replace(false)) {
        return;
    }
    DOCUMENT.with(|document| {
        if let Some(document) = document.borrow().as_ref() {
            document.doc_mut().resolve(started.elapsed().as_secs_f64());
        }
    });
}

/// The app's icon, the loading screen's clown (see assets/CREDITS.md), for the window's corner
/// and the task bar. None if it cannot be read, and the system's own is shown.
fn window_icon() -> Option<dioxus_native::winit::icon::Icon> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(&include_bytes!("../assets/icon.png")[..])).read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut rgba).ok()?;
    rgba.truncate(frame.buffer_size());
    let icon = dioxus_native::winit::icon::RgbaIcon::new(rgba, frame.width, frame.height).ok()?;
    Some(icon.into())
}

pub(crate) fn launch(app: fn() -> Element) {
    let window = dioxus_native::WindowAttributes::default()
        .with_title(crate::TITLE)
        .with_surface_size(dioxus_native::LogicalSize::new(1600.0, 900.0))
        .with_window_icon(window_icon());
    let event_loop = blitz_shell::create_default_event_loop();
    let (proxy, event_queue) = blitz_shell::BlitzShellProxy::new(event_loop.create_proxy());

    // Blitz fetches pictures (here the palettes' data addresses) on Tokio.
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio");
    let _guard = runtime.enter();

    let vdom = VirtualDom::new(app);
    let net: Arc<dyn NetProvider> = blitz_net::Provider::shared(Some(Arc::new(proxy.clone()) as _));
    vdom.provide_root_context(Arc::clone(&net));
    let doc = DioxusDocument::new(vdom, DocumentConfig { net_provider: Some(net), ..Default::default() });
    let renderer = DioxusNativeWindowRenderer::with_options(RendererOptions { base_color: None, alpha_mode: None, features: None, limits: None });
    let config = blitz_shell::WindowConfig::with_attributes(Box::new(doc) as _, renderer, window);
    let application = DioxusNativeApplication::new(proxy.clone(), event_queue, config);
    event_loop.run_app(Timed { inner: application, proxy, window: None, started: Instant::now() }).expect("event loop");
}

/// Dioxus Native's application, with its redraws timed and its close held back.
struct Timed {
    inner: DioxusNativeApplication,
    /// For asking the document to draw its components again before a frame.
    proxy: blitz_shell::BlitzShellProxy,
    window: Option<WindowId>,
    started: Instant,
}

impl Timed {
    /// Closes the window once the user has agreed to lose unsaved changes.
    fn close_if_quitting(&mut self, event_loop: &dyn ActiveEventLoop) {
        let quitting = crate::try_with(|editor| std::mem::take(&mut editor.quitting));
        if let (Some(true), Some(window)) = (quitting, self.window) {
            self.inner.window_event(event_loop, window, WindowEvent::CloseRequested);
        }
    }
}

impl ApplicationHandler for Timed {
    fn new_events(&mut self, event_loop: &dyn ActiveEventLoop, cause: StartCause) {
        self.inner.new_events(event_loop, cause);
    }

    fn resumed(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.resumed(event_loop);
    }

    fn suspended(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.suspended(event_loop);
    }

    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.can_create_surfaces(event_loop);
    }

    fn destroy_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.destroy_surfaces(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.about_to_wait(event_loop);
        self.close_if_quitting(event_loop);
    }

    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.proxy_wake_up(event_loop);
        self.close_if_quitting(event_loop);
    }

    fn window_event(&mut self, event_loop: &dyn ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        self.window = Some(window_id);
        match event {
            // Closing the window with unsaved changes asks first.
            WindowEvent::CloseRequested if crate::try_with(|editor| editor.dirty()) == Some(true) => {
                crate::with(|editor| editor.confirm = Some(Discarding::Quit));
                crate::bump();
            }
            WindowEvent::RedrawRequested => {
                let start = Instant::now();
                // The entity palette's rows follow its list's scroll in this frame: the
                // components are drawn again now, before the frame, rather than after it.
                if crate::palettes::follow_entity_scroll() {
                    self.proxy.send_event(blitz_shell::BlitzShellEvent::Poll { window_id });
                    self.inner.proxy_wake_up(event_loop);
                }
                self.inner.window_event(event_loop, window_id, event);
                // A scroll made while the frame was drawn (an animation, or a list grown
                // shorter) shows in the next frame: the poll sent after each event draws it.
                crate::palettes::follow_entity_scroll();
                let cost = start.elapsed().as_secs_f64();
                STALE.with(|stale| stale.set(false));
                FRAME_TIMES.with(|times| {
                    let mut times = times.borrow_mut();
                    if times.costs.len() == FRAMES {
                        times.costs.pop_front();
                        times.starts.pop_front();
                    }
                    times.costs.push_back(cost);
                    times.starts.push_back(start);
                    times.drawn += 1;
                });
            }
            event @ (WindowEvent::PointerMoved { .. } | WindowEvent::PointerButton { .. } | WindowEvent::PointerLeft { .. } | WindowEvent::PointerEntered { .. } | WindowEvent::MouseWheel { .. }) => {
                settle(self.started);
                self.inner.window_event(event_loop, window_id, event);
            }
            event => self.inner.window_event(event_loop, window_id, event),
        }
        self.close_if_quitting(event_loop);
    }
}
