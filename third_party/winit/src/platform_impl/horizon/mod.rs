//! Nintendo Switch homebrew (libnx) backend, added by Horizon's port (docs/switch.md).
//!
//! The Switch has one screen and no window system: every window is libnx's default
//! `NWindow`, 1280x720 (the system scales it to the TV when docked), handed to EGL as an
//! Android native window. The controllers are read by the game itself; this backend sends
//! the touch screen as the mouse, the HOME menu's focus and exit requests, and opens the
//! system keyboard (swkbd) while the game allows text input (`set_ime_allowed`).
#![cfg(target_os = "horizon")]

use std::fmt::{self, Display, Formatter};

use smol_str::SmolStr;

use crate::dpi::{PhysicalPosition, PhysicalSize};
use crate::keyboard::Key;

pub(crate) use self::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy, OwnedDisplayHandle};
mod event_loop;

pub use self::window::Window;
mod window;

/// The size of libnx's default window, in pixels.
pub(crate) const SCREEN: (u32, u32) = (1280, 720);

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PlatformSpecificEventLoopAttributes {}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WindowId(u64);

impl WindowId {
    pub const fn dummy() -> Self {
        WindowId(u64::MAX)
    }
}

impl From<WindowId> for u64 {
    fn from(id: WindowId) -> Self {
        id.0
    }
}

impl From<u64> for WindowId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DeviceId;

impl DeviceId {
    pub const fn dummy() -> Self {
        DeviceId
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlatformSpecificWindowAttributes;

#[derive(Clone, Debug)]
pub struct OsError(String);

impl Display for OsError {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        fmt.write_str(&self.0)
    }
}

pub(crate) use crate::cursor::{
    NoCustomCursor as PlatformCustomCursor, NoCustomCursor as PlatformCustomCursorSource,
};
pub(crate) use crate::icon::NoIcon as PlatformIcon;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MonitorHandle;

impl MonitorHandle {
    pub fn name(&self) -> Option<String> {
        Some("Nintendo Switch".to_owned())
    }

    pub fn size(&self) -> PhysicalSize<u32> {
        SCREEN.into()
    }

    pub fn position(&self) -> PhysicalPosition<i32> {
        (0, 0).into()
    }

    pub fn scale_factor(&self) -> f64 {
        1.0
    }

    pub fn refresh_rate_millihertz(&self) -> Option<u32> {
        Some(60000)
    }

    pub fn video_modes(&self) -> impl Iterator<Item = VideoModeHandle> {
        std::iter::once(VideoModeHandle { monitor: self.clone() })
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct VideoModeHandle {
    monitor: MonitorHandle,
}

impl VideoModeHandle {
    pub fn size(&self) -> PhysicalSize<u32> {
        SCREEN.into()
    }

    pub fn bit_depth(&self) -> u16 {
        32
    }

    pub fn refresh_rate_millihertz(&self) -> u32 {
        60000
    }

    pub fn monitor(&self) -> MonitorHandle {
        self.monitor.clone()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct KeyEventExtra {
    pub key_without_modifiers: Key,
    pub text_with_all_modifiers: Option<SmolStr>,
}
