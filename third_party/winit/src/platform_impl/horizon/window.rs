use std::collections::VecDeque;
use std::sync::Arc;

use crate::cursor::Cursor;
use crate::dpi::{PhysicalPosition, PhysicalSize, Position, Size};
use crate::error;
use crate::platform_impl::Fullscreen;
use crate::window::{self, ImePurpose};

use super::event_loop::Shared;
use super::{ActiveEventLoop, MonitorHandle, WindowId, SCREEN};

pub struct Window {
    id: WindowId,
    shared: Arc<Shared>,
    title: std::sync::Mutex<String>,
}

impl Window {
    pub(crate) fn new(
        el: &ActiveEventLoop,
        attrs: window::WindowAttributes,
    ) -> Result<Self, error::OsError> {
        let shared = el.shared.clone();
        let id = WindowId(shared.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        shared.creates.lock().unwrap().push_back(id);
        shared.wake();
        Ok(Self { id, shared, title: std::sync::Mutex::new(attrs.title) })
    }

    pub(crate) fn maybe_queue_on_main(&self, f: impl FnOnce(&Self) + Send + 'static) {
        f(self)
    }

    pub(crate) fn maybe_wait_on_main<R: Send>(&self, f: impl FnOnce(&Self) -> R + Send) -> R {
        f(self)
    }

    #[inline]
    pub fn id(&self) -> WindowId {
        self.id
    }

    #[inline]
    pub fn primary_monitor(&self) -> Option<MonitorHandle> {
        Some(MonitorHandle)
    }

    #[inline]
    pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
        VecDeque::from([MonitorHandle])
    }

    #[inline]
    pub fn current_monitor(&self) -> Option<MonitorHandle> {
        Some(MonitorHandle)
    }

    #[inline]
    pub fn scale_factor(&self) -> f64 {
        1.0
    }

    #[inline]
    pub fn request_redraw(&self) {
        let mut redraws = self.shared.redraws.lock().unwrap();
        if !redraws.contains(&self.id) {
            redraws.push_back(self.id);
            drop(redraws);
            self.shared.wake();
        }
    }

    #[inline]
    pub fn pre_present_notify(&self) {}

    #[inline]
    pub fn reset_dead_keys(&self) {}

    #[inline]
    pub fn inner_position(&self) -> Result<PhysicalPosition<i32>, error::NotSupportedError> {
        Ok((0, 0).into())
    }

    #[inline]
    pub fn outer_position(&self) -> Result<PhysicalPosition<i32>, error::NotSupportedError> {
        Ok((0, 0).into())
    }

    #[inline]
    pub fn set_outer_position(&self, _position: Position) {}

    #[inline]
    pub fn inner_size(&self) -> PhysicalSize<u32> {
        SCREEN.into()
    }

    #[inline]
    pub fn request_inner_size(&self, _size: Size) -> Option<PhysicalSize<u32>> {
        Some(SCREEN.into())
    }

    #[inline]
    pub fn outer_size(&self) -> PhysicalSize<u32> {
        SCREEN.into()
    }

    #[inline]
    pub fn set_min_inner_size(&self, _: Option<Size>) {}

    #[inline]
    pub fn set_max_inner_size(&self, _: Option<Size>) {}

    #[inline]
    pub fn title(&self) -> String {
        self.title.lock().unwrap().clone()
    }

    #[inline]
    pub fn set_title(&self, title: &str) {
        *self.title.lock().unwrap() = title.to_owned();
    }

    #[inline]
    pub fn set_transparent(&self, _transparent: bool) {}

    #[inline]
    pub fn set_blur(&self, _blur: bool) {}

    #[inline]
    pub fn set_visible(&self, _visible: bool) {}

    #[inline]
    pub fn is_visible(&self) -> Option<bool> {
        Some(true)
    }

    #[inline]
    pub fn resize_increments(&self) -> Option<PhysicalSize<u32>> {
        None
    }

    #[inline]
    pub fn set_resize_increments(&self, _increments: Option<Size>) {}

    #[inline]
    pub fn set_resizable(&self, _resizeable: bool) {}

    #[inline]
    pub fn is_resizable(&self) -> bool {
        false
    }

    #[inline]
    pub fn set_minimized(&self, _minimized: bool) {}

    #[inline]
    pub fn is_minimized(&self) -> Option<bool> {
        Some(false)
    }

    #[inline]
    pub fn set_maximized(&self, _maximized: bool) {}

    #[inline]
    pub fn is_maximized(&self) -> bool {
        true
    }

    #[inline]
    pub(crate) fn set_fullscreen(&self, _monitor: Option<Fullscreen>) {}

    #[inline]
    pub(crate) fn fullscreen(&self) -> Option<Fullscreen> {
        Some(Fullscreen::Borderless(Some(MonitorHandle)))
    }

    #[inline]
    pub fn set_decorations(&self, _decorations: bool) {}

    #[inline]
    pub fn is_decorated(&self) -> bool {
        false
    }

    #[inline]
    pub fn set_window_level(&self, _level: window::WindowLevel) {}

    #[inline]
    pub fn set_window_icon(&self, _window_icon: Option<crate::icon::Icon>) {}

    #[inline]
    pub fn set_ime_cursor_area(&self, _position: Position, _size: Size) {}

    /// The game allows text input while a text box has focus: the system keyboard opens
    /// at the end of this loop iteration and its text arrives as `Ime::Commit`.
    #[inline]
    pub fn set_ime_allowed(&self, allowed: bool) {
        self.shared.ime_requests.lock().unwrap().push_back((self.id, allowed));
        self.shared.wake();
    }

    #[inline]
    pub fn set_ime_purpose(&self, _purpose: ImePurpose) {}

    #[inline]
    pub fn focus_window(&self) {}

    #[inline]
    pub fn request_user_attention(&self, _request_type: Option<window::UserAttentionType>) {}

    #[inline]
    pub fn set_cursor(&self, _: Cursor) {}

    #[inline]
    pub fn set_cursor_position(&self, _: Position) -> Result<(), error::ExternalError> {
        Ok(())
    }

    #[inline]
    pub fn set_cursor_grab(&self, _mode: window::CursorGrabMode) -> Result<(), error::ExternalError> {
        Ok(())
    }

    #[inline]
    pub fn set_cursor_visible(&self, _visible: bool) {}

    #[inline]
    pub fn drag_window(&self) -> Result<(), error::ExternalError> {
        Err(error::ExternalError::NotSupported(error::NotSupportedError::new()))
    }

    #[inline]
    pub fn drag_resize_window(
        &self,
        _direction: window::ResizeDirection,
    ) -> Result<(), error::ExternalError> {
        Err(error::ExternalError::NotSupported(error::NotSupportedError::new()))
    }

    #[inline]
    pub fn show_window_menu(&self, _position: Position) {}

    #[inline]
    pub fn set_cursor_hittest(&self, _hittest: bool) -> Result<(), error::ExternalError> {
        Err(error::ExternalError::NotSupported(error::NotSupportedError::new()))
    }

    #[cfg(feature = "rwh_04")]
    #[inline]
    pub fn raw_window_handle_rwh_04(&self) -> rwh_04::RawWindowHandle {
        let mut handle = rwh_04::AndroidNdkHandle::empty();
        handle.a_native_window = horizon_switch::default_window();
        rwh_04::RawWindowHandle::AndroidNdk(handle)
    }

    #[cfg(feature = "rwh_05")]
    #[inline]
    pub fn raw_window_handle_rwh_05(&self) -> rwh_05::RawWindowHandle {
        let mut handle = rwh_05::AndroidNdkWindowHandle::empty();
        handle.a_native_window = horizon_switch::default_window();
        rwh_05::RawWindowHandle::AndroidNdk(handle)
    }

    #[cfg(feature = "rwh_05")]
    #[inline]
    pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
        rwh_05::RawDisplayHandle::Android(rwh_05::AndroidDisplayHandle::empty())
    }

    /// libnx's `NWindow*`, as an Android native window (what EGL wants).
    #[cfg(feature = "rwh_06")]
    #[inline]
    pub fn raw_window_handle_rwh_06(&self) -> Result<rwh_06::RawWindowHandle, rwh_06::HandleError> {
        let window = std::ptr::NonNull::new(horizon_switch::default_window())
            .ok_or(rwh_06::HandleError::Unavailable)?;
        Ok(rwh_06::RawWindowHandle::AndroidNdk(rwh_06::AndroidNdkWindowHandle::new(window)))
    }

    #[cfg(feature = "rwh_06")]
    #[inline]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::RawDisplayHandle::Android(rwh_06::AndroidDisplayHandle::new()))
    }

    #[inline]
    pub fn set_enabled_buttons(&self, _buttons: window::WindowButtons) {}

    #[inline]
    pub fn enabled_buttons(&self) -> window::WindowButtons {
        window::WindowButtons::empty()
    }

    #[inline]
    pub fn theme(&self) -> Option<window::Theme> {
        Some(window::Theme::Dark)
    }

    #[inline]
    pub fn has_focus(&self) -> bool {
        horizon_switch::focused()
    }

    #[inline]
    pub fn set_theme(&self, _theme: Option<window::Theme>) {}

    pub fn set_content_protected(&self, _protected: bool) {}
}

impl Drop for Window {
    fn drop(&mut self) {
        self.shared.destroys.lock().unwrap().push_back(self.id);
        self.shared.wake();
    }
}
