use std::cell::Cell;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::sync::atomic::AtomicU64;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::error::EventLoopError;
use crate::event::{self, Ime, StartCause};
use crate::keyboard::{Key, KeyCode, KeyLocation, NamedKey, PhysicalKey};
use crate::event_loop::{self, ControlFlow, DeviceEvents};
use crate::window::{CustomCursor as RootCustomCursor, CustomCursorSource, Theme, WindowId as RootWindowId};

use super::{DeviceId, KeyEventExtra, MonitorHandle, PlatformSpecificEventLoopAttributes, WindowId, SCREEN};

/// The controller as keys, for the screens that take only keys (the game's startup menu:
/// title, world creation, options). In a world the game reads the pad itself
/// (`horizon_switch::input::game_reads_pad`) and this stays quiet.
const PAD_KEYS: [(u64, KeyCode, NamedKey); 8] = {
    use horizon_switch::input::button as b;
    [
        (b::UP | b::STICK_L_UP, KeyCode::ArrowUp, NamedKey::ArrowUp),
        (b::DOWN | b::STICK_L_DOWN, KeyCode::ArrowDown, NamedKey::ArrowDown),
        (b::LEFT | b::STICK_L_LEFT, KeyCode::ArrowLeft, NamedKey::ArrowLeft),
        (b::RIGHT | b::STICK_L_RIGHT, KeyCode::ArrowRight, NamedKey::ArrowRight),
        (b::A, KeyCode::Enter, NamedKey::Enter),
        (b::B, KeyCode::Escape, NamedKey::Escape),
        (b::X, KeyCode::Tab, NamedKey::Tab),
        (b::Y, KeyCode::Space, NamedKey::Space),
    ]
};
/// Arrows held repeat, as a keyboard's do.
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_EVERY: Duration = Duration::from_millis(110);

#[derive(Default)]
struct PadKeys {
    /// Which of `PAD_KEYS` are down, and when each next repeats.
    down: [Option<Instant>; 8],
}

fn key_event(code: KeyCode, named: NamedKey, pressed: bool, repeat: bool) -> event::KeyEvent {
    let logical = Key::Named(named);
    let text = match (pressed, named) {
        (true, NamedKey::Space) => Some(" ".into()),
        _ => None,
    };
    event::KeyEvent {
        physical_key: PhysicalKey::Code(code),
        logical_key: logical.clone(),
        text,
        location: KeyLocation::Standard,
        state: if pressed { event::ElementState::Pressed } else { event::ElementState::Released },
        repeat,
        platform_specific: KeyEventExtra { key_without_modifiers: logical, text_with_all_modifiers: None },
    }
}

/// What windows and proxies hand to the loop, from any thread.
pub(super) struct Shared {
    pub(super) next_id: AtomicU64,
    pub(super) creates: Mutex<VecDeque<WindowId>>,
    pub(super) redraws: Mutex<VecDeque<WindowId>>,
    pub(super) destroys: Mutex<VecDeque<WindowId>>,
    /// `set_ime_allowed` calls, in order.
    pub(super) ime_requests: Mutex<VecDeque<(WindowId, bool)>>,
    woken: Mutex<bool>,
    wake_cv: Condvar,
}

impl Shared {
    fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            creates: Mutex::new(VecDeque::new()),
            redraws: Mutex::new(VecDeque::new()),
            destroys: Mutex::new(VecDeque::new()),
            ime_requests: Mutex::new(VecDeque::new()),
            woken: Mutex::new(false),
            wake_cv: Condvar::new(),
        }
    }

    /// Ends a `Wait`/`WaitUntil` early.
    pub(super) fn wake(&self) {
        *self.woken.lock().unwrap() = true;
        self.wake_cv.notify_one();
    }

    /// Sleeps until woken or `deadline`, never longer than `cap` (the HOME menu and the
    /// touch screen are polled, not signalled). True when woken.
    fn wait(&self, deadline: Option<Instant>, cap: Duration) -> bool {
        let now = Instant::now();
        let mut until = now + cap;
        if let Some(deadline) = deadline {
            until = until.min(deadline);
        }
        let mut woken = self.woken.lock().unwrap();
        while !*woken {
            let now = Instant::now();
            if now >= until {
                break;
            }
            woken = self.wake_cv.wait_timeout(woken, until - now).unwrap().0;
        }
        std::mem::take(&mut *woken)
    }
}

/// The touch screen as a mouse: one finger moves the pointer and holds the left button.
#[derive(Default)]
struct Touch {
    finger: Option<u32>,
    position: (f64, f64),
}

pub struct EventLoop<T> {
    windows: Vec<WindowId>,
    window_target: event_loop::ActiveEventLoop,
    user_events_sender: mpsc::Sender<T>,
    user_events_receiver: mpsc::Receiver<T>,
    touch: Touch,
    focused: Option<bool>,
    /// The windows that allow text input, and those whose keyboard is still to open.
    ime_allowed: Vec<WindowId>,
    ime_pending: Vec<WindowId>,
    /// Iterations since the system asked the program to quit.
    exit_requested: Option<u32>,
    pad_keys: PadKeys,
}

impl<T: 'static> EventLoop<T> {
    pub(crate) fn new(_: &PlatformSpecificEventLoopAttributes) -> Result<Self, EventLoopError> {
        let (user_events_sender, user_events_receiver) = mpsc::channel();
        Ok(Self {
            windows: Vec::new(),
            window_target: event_loop::ActiveEventLoop {
                p: ActiveEventLoop {
                    control_flow: Cell::new(ControlFlow::default()),
                    exit: Cell::new(false),
                    shared: Arc::new(Shared::new()),
                },
                _marker: PhantomData,
            },
            user_events_sender,
            user_events_receiver,
            touch: Touch::default(),
            focused: None,
            ime_allowed: Vec::new(),
            ime_pending: Vec::new(),
            exit_requested: None,
            pad_keys: PadKeys::default(),
        })
    }

    fn window_event<F>(&self, handler: &mut F, window_id: WindowId, event: event::WindowEvent)
    where
        F: FnMut(event::Event<T>, &event_loop::ActiveEventLoop),
    {
        handler(event::Event::WindowEvent { window_id: RootWindowId(window_id), event }, &self.window_target);
    }

    /// The window input goes to: the newest.
    fn front(&self) -> Option<WindowId> {
        self.windows.last().copied()
    }

    fn poll_touch<F>(&mut self, handler: &mut F)
    where
        F: FnMut(event::Event<T>, &event_loop::ActiveEventLoop),
    {
        let Some(window_id) = self.front() else { return };
        // In a world the game's on-screen controls read the touch screen themselves.
        if horizon_switch::input::game_owns_touch() {
            if self.touch.finger.take().is_some() {
                self.window_event(handler, window_id, event::WindowEvent::MouseInput {
                    device_id: event::DeviceId(DeviceId),
                    state: event::ElementState::Released,
                    button: event::MouseButton::Left,
                });
            }
            return;
        }
        let state = horizon_switch::input::poll_touch();
        let finger = (0..state.touch_count as usize)
            .find(|&i| Some(state.touch_id[i]) == self.touch.finger)
            .or((state.touch_count > 0).then_some(0));
        match (self.touch.finger, finger) {
            (_, Some(i)) => {
                // Touch coordinates are the screen's 1280x720 in both modes.
                let x = f64::from(state.touch_x[i]) * f64::from(SCREEN.0) / 1280.0;
                let y = f64::from(state.touch_y[i]) * f64::from(SCREEN.1) / 720.0;
                let pressed_now = self.touch.finger.is_none();
                if pressed_now || (x, y) != self.touch.position {
                    self.touch.position = (x, y);
                    self.window_event(handler, window_id, event::WindowEvent::CursorMoved {
                        device_id: event::DeviceId(DeviceId),
                        position: (x, y).into(),
                    });
                }
                if pressed_now {
                    self.touch.finger = Some(state.touch_id[i]);
                    self.window_event(handler, window_id, event::WindowEvent::MouseInput {
                        device_id: event::DeviceId(DeviceId),
                        state: event::ElementState::Pressed,
                        button: event::MouseButton::Left,
                    });
                }
            },
            (Some(_), None) => {
                self.touch.finger = None;
                self.window_event(handler, window_id, event::WindowEvent::MouseInput {
                    device_id: event::DeviceId(DeviceId),
                    state: event::ElementState::Released,
                    button: event::MouseButton::Left,
                });
            },
            (None, None) => {},
        }
    }

    /// The controller as keys while the game does not read it itself.
    fn poll_pad_keys<F>(&mut self, handler: &mut F)
    where
        F: FnMut(event::Event<T>, &event_loop::ActiveEventLoop),
    {
        let Some(window_id) = self.front() else { return };
        let held = if horizon_switch::input::game_reads_pad() {
            0
        } else {
            horizon_switch::input::poll().held
        };
        let now = Instant::now();
        for (i, (bits, code, named)) in PAD_KEYS.into_iter().enumerate() {
            let pressed = held & bits != 0;
            let arrow = i < 4;
            let event = match (self.pad_keys.down[i], pressed) {
                (None, true) => {
                    self.pad_keys.down[i] = Some(now + REPEAT_DELAY);
                    Some(key_event(code, named, true, false))
                },
                (Some(next), true) if arrow && now >= next => {
                    self.pad_keys.down[i] = Some(now + REPEAT_EVERY);
                    Some(key_event(code, named, true, true))
                },
                (Some(_), false) => {
                    self.pad_keys.down[i] = None;
                    Some(key_event(code, named, false, false))
                },
                _ => None,
            };
            if let Some(event) = event {
                self.window_event(handler, window_id, event::WindowEvent::KeyboardInput {
                    device_id: event::DeviceId(DeviceId),
                    event,
                    is_synthetic: false,
                });
            }
        }
    }

    /// Opens the system keyboard for a window that allows text input.
    fn run_keyboard<F>(&mut self, handler: &mut F)
    where
        F: FnMut(event::Event<T>, &event_loop::ActiveEventLoop),
    {
        while let Some(window_id) = self.ime_pending.pop() {
            if !self.ime_allowed.contains(&window_id) || !self.windows.contains(&window_id) {
                continue;
            }
            // Blocks while the keyboard applet is up.
            if let Some(text) = horizon_switch::keyboard("", "") {
                if !text.is_empty() {
                    self.window_event(handler, window_id, event::WindowEvent::Ime(Ime::Preedit(String::new(), None)));
                    self.window_event(handler, window_id, event::WindowEvent::Ime(Ime::Commit(text)));
                }
            }
        }
    }

    pub fn run<F>(mut self, mut event_handler: F) -> Result<(), EventLoopError>
    where
        F: FnMut(event::Event<T>, &event_loop::ActiveEventLoop),
    {
        let mut start_cause = StartCause::Init;
        loop {
            event_handler(event::Event::NewEvents(start_cause), &self.window_target);
            if start_cause == StartCause::Init {
                event_handler(event::Event::Resumed, &self.window_target);
            }

            // libnx's applet loop: the HOME menu, sleep, the docked/handheld switch. False
            // once the system asks the program to quit.
            let running = horizon_switch::main_loop();

            let creates: Vec<_> = self.window_target.p.shared.creates.lock().unwrap().drain(..).collect();
            for window_id in creates {
                self.windows.push(window_id);
                self.window_event(&mut event_handler, window_id, event::WindowEvent::Resized(SCREEN.into()));
                self.window_event(&mut event_handler, window_id, event::WindowEvent::Moved((0, 0).into()));
                self.window_event(&mut event_handler, window_id, event::WindowEvent::Focused(true));
                self.window_target.p.shared.redraws.lock().unwrap().push_back(window_id);
            }

            let destroys: Vec<_> = self.window_target.p.shared.destroys.lock().unwrap().drain(..).collect();
            for window_id in destroys {
                self.window_event(&mut event_handler, window_id, event::WindowEvent::Destroyed);
                self.windows.retain(|w| *w != window_id);
                self.ime_allowed.retain(|w| *w != window_id);
            }

            let focused = horizon_switch::focused();
            if self.focused != Some(focused) {
                self.focused = Some(focused);
                for window_id in self.windows.clone() {
                    self.window_event(&mut event_handler, window_id, event::WindowEvent::Focused(focused));
                }
            }

            if focused {
                self.poll_touch(&mut event_handler);
                self.poll_pad_keys(&mut event_handler);
            }

            if !running {
                let n = self.exit_requested.get_or_insert(0);
                *n += 1;
                let n = *n;
                if n == 1 {
                    eprintln!("Horizon: the system asked the program to quit");
                }
                for window_id in self.windows.clone() {
                    self.window_event(&mut event_handler, window_id, event::WindowEvent::CloseRequested);
                }
                // The system closes a program that does not quit soon enough.
                if n > 120 || self.windows.is_empty() {
                    self.window_target.p.exit();
                }
            }

            while let Ok(event) = self.user_events_receiver.try_recv() {
                event_handler(event::Event::UserEvent(event), &self.window_target);
            }

            let ime: Vec<_> = self.window_target.p.shared.ime_requests.lock().unwrap().drain(..).collect();
            for (window_id, allowed) in ime {
                let was = self.ime_allowed.contains(&window_id);
                if allowed && !was {
                    self.ime_allowed.push(window_id);
                    self.ime_pending.push(window_id);
                    self.window_event(&mut event_handler, window_id, event::WindowEvent::Ime(Ime::Enabled));
                } else if !allowed && was {
                    self.ime_allowed.retain(|w| *w != window_id);
                    self.ime_pending.retain(|w| *w != window_id);
                    self.window_event(&mut event_handler, window_id, event::WindowEvent::Ime(Ime::Disabled));
                }
            }

            loop {
                let next = self.window_target.p.shared.redraws.lock().unwrap().pop_front();
                let Some(window_id) = next else { break };
                if self.windows.contains(&window_id) {
                    self.window_event(&mut event_handler, window_id, event::WindowEvent::RedrawRequested);
                }
            }

            event_handler(event::Event::AboutToWait, &self.window_target);

            if !self.ime_pending.is_empty() && focused {
                self.run_keyboard(&mut event_handler);
            }

            if self.window_target.p.exiting() {
                break;
            }

            let start = Instant::now();
            start_cause = match self.window_target.p.control_flow() {
                ControlFlow::Poll => StartCause::Poll,
                ControlFlow::Wait => {
                    let woken = self.window_target.p.shared.wait(None, Duration::from_millis(16));
                    let _ = woken;
                    StartCause::WaitCancelled { start, requested_resume: None }
                },
                ControlFlow::WaitUntil(requested_resume) => {
                    let woken = self.window_target.p.shared.wait(Some(requested_resume), Duration::from_millis(16));
                    if !woken && Instant::now() >= requested_resume {
                        StartCause::ResumeTimeReached { start, requested_resume }
                    } else {
                        StartCause::WaitCancelled { start, requested_resume: Some(requested_resume) }
                    }
                },
            };
        }

        event_handler(event::Event::LoopExiting, &self.window_target);
        Ok(())
    }

    pub fn window_target(&self) -> &event_loop::ActiveEventLoop {
        &self.window_target
    }

    pub fn create_proxy(&self) -> EventLoopProxy<T> {
        EventLoopProxy {
            user_events_sender: self.user_events_sender.clone(),
            shared: self.window_target.p.shared.clone(),
        }
    }
}

pub struct EventLoopProxy<T: 'static> {
    user_events_sender: mpsc::Sender<T>,
    shared: Arc<Shared>,
}

impl<T> EventLoopProxy<T> {
    pub fn send_event(&self, event: T) -> Result<(), event_loop::EventLoopClosed<T>> {
        self.user_events_sender
            .send(event)
            .map_err(|mpsc::SendError(x)| event_loop::EventLoopClosed(x))?;
        self.shared.wake();
        Ok(())
    }
}

impl<T> Clone for EventLoopProxy<T> {
    fn clone(&self) -> Self {
        Self { user_events_sender: self.user_events_sender.clone(), shared: self.shared.clone() }
    }
}

impl<T> Unpin for EventLoopProxy<T> {}

pub struct ActiveEventLoop {
    control_flow: Cell<ControlFlow>,
    exit: Cell<bool>,
    pub(super) shared: Arc<Shared>,
}

impl ActiveEventLoop {
    pub fn create_custom_cursor(&self, source: CustomCursorSource) -> RootCustomCursor {
        let _ = source.inner;
        RootCustomCursor { inner: super::PlatformCustomCursor }
    }

    pub fn primary_monitor(&self) -> Option<MonitorHandle> {
        Some(MonitorHandle)
    }

    pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
        VecDeque::from([MonitorHandle])
    }

    #[inline]
    pub fn listen_device_events(&self, _allowed: DeviceEvents) {}

    #[cfg(feature = "rwh_05")]
    #[inline]
    pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
        rwh_05::RawDisplayHandle::Android(rwh_05::AndroidDisplayHandle::empty())
    }

    #[inline]
    pub fn system_theme(&self) -> Option<Theme> {
        Some(Theme::Dark)
    }

    #[cfg(feature = "rwh_06")]
    #[inline]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::RawDisplayHandle::Android(rwh_06::AndroidDisplayHandle::new()))
    }

    pub fn set_control_flow(&self, control_flow: ControlFlow) {
        self.control_flow.set(control_flow)
    }

    pub fn control_flow(&self) -> ControlFlow {
        self.control_flow.get()
    }

    pub(crate) fn exit(&self) {
        self.exit.set(true);
    }

    pub(crate) fn exiting(&self) -> bool {
        self.exit.get()
    }

    pub(crate) fn owned_display_handle(&self) -> OwnedDisplayHandle {
        OwnedDisplayHandle
    }
}

#[derive(Clone)]
pub(crate) struct OwnedDisplayHandle;

impl OwnedDisplayHandle {
    #[cfg(feature = "rwh_05")]
    #[inline]
    pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
        rwh_05::AndroidDisplayHandle::empty().into()
    }

    #[cfg(feature = "rwh_06")]
    #[inline]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::AndroidDisplayHandle::new().into())
    }
}
