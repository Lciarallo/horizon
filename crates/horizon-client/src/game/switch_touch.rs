//! The Nintendo Switch's touch screen in a world (docs/switch.md): the on-screen controls of
//! Horizon's Android work (`horizon_client::touch`): a joystick, jump, sneak, sprint, attack,
//! use, inventory, chat, pause, camera and swap hands, and a drag on the rest of the screen
//! turning the view. They show once the screen is touched in a world and hide again when a
//! controller is used, as Android's automatic input mode does. In menus the touch screen
//! stays the mouse (winit's backend).
use super::{Game, Screen};
use horizon_client::gamepad::PadState;
use horizon_client::hud::HudBatch;
use horizon_client_render::touch::{self, Control, TouchControls};
use std::sync::Mutex;
use winit::keyboard::KeyCode;

/// The touch screen's size in its own coordinates (both modes).
const SURFACE: (u32, u32) = (1280, 720);

struct State {
    controls: TouchControls,
    /// Fingers down at the last frame: libnx id and position (0..1).
    fingers: Vec<(u32, [f32; 2])>,
    /// The controls show: the screen was touched after the controller was last used.
    shown: bool,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn new_state() -> State {
    let mut controls = TouchControls::default();
    controls.viewport(SURFACE.0, SURFACE.1);
    // F3 is key combinations; the performance overlay is in Video Settings.
    controls.settings.button_visibility[Control::Debug as usize] = touch::layout::HIDDEN;
    State { controls, fingers: Vec::new(), shown: false }
}

impl Game<'_> {
    /// Whether the in-world touch controls take the touch screen now.
    fn switch_touch_in_world(&self) -> bool {
        self.screen == Screen::Playing && self.player.is_some() && !self.pad_in_menu()
    }

    /// This frame's touches in a world: the controller state the on-screen controls hold,
    /// to merge with the physical controller's (`hardware`). Turns the view and runs the
    /// shortcut buttons' keys itself.
    pub(super) fn switch_touch_frame(&mut self, hardware: &PadState) -> PadState {
        let mut guard = STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = guard.get_or_insert_with(new_state);
        if !self.switch_touch_in_world() {
            if !state.fingers.is_empty() {
                state.controls.clear();
                state.fingers.clear();
            }
            return PadState::default();
        }
        horizon_switch::input::note_game_touch();
        // A controller in use hides the controls (its sticks past a small deadzone).
        let stick = |s: [f32; 2]| s[0].hypot(s[1]) > 0.3;
        if hardware.buttons != 0 || stick(hardware.left) || stick(hardware.right) {
            state.shown = false;
        }
        let input = horizon_switch::input::poll_touch();
        let now: Vec<(u32, [f32; 2])> = (0..input.touch_count as usize)
            .map(|i| {
                let p = [
                    input.touch_x[i] as f32 / SURFACE.0 as f32,
                    input.touch_y[i] as f32 / SURFACE.1 as f32,
                ];
                (input.touch_id[i], p)
            })
            .collect();
        if !now.is_empty() {
            state.shown = true;
        }
        let mut turn = [0.0f32; 2];
        for &(id, last) in &state.fingers {
            if !now.iter().any(|(n, _)| *n == id) {
                state.controls.update(u64::from(id), last, false, true);
            }
        }
        for &(id, point) in &now {
            let started = !state.fingers.iter().any(|(o, _)| *o == id);
            let d = state.controls.update(u64::from(id), point, started, false);
            turn[0] += d[0];
            turn[1] += d[1];
        }
        state.fingers = now;
        let pad = state.controls.poll_state();
        let mut keys = Vec::new();
        if std::mem::take(&mut state.controls.chat_requested) {
            keys.push(KeyCode::KeyT);
        }
        if std::mem::take(&mut state.controls.settings_requested) {
            keys.push(KeyCode::Escape);
        }
        if std::mem::take(&mut state.controls.perspective_requested) {
            keys.push(KeyCode::F5);
        }
        state.controls.debug_requested = false;
        for (token, down) in state.controls.key_events() {
            if down && token == "action:key.swapOffhand" {
                keys.push(KeyCode::KeyF);
            }
        }
        state.controls.clicked = false;
        let degrees = touch::camera_delta(turn, SURFACE.0, SURFACE.1, &state.controls.settings);
        drop(guard);
        if degrees != [0.0; 2] {
            // `Entity.turn` units: 0.15 degrees each.
            self.look_turn(f64::from(degrees[0]) / 0.15, f64::from(degrees[1]) / 0.15);
        }
        for key in keys {
            if let Err(e) = self.key_pressed(key, key) {
                log::warn!("Horizon: touch control: {e}");
            }
        }
        pad
    }

    /// The on-screen controls over the HUD, while they show.
    pub(super) fn draw_switch_touch(&self, batch: &mut HudBatch) {
        if !self.switch_touch_in_world() {
            return;
        }
        let guard = STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(state) = guard.as_ref()
            && state.shown
        {
            state.controls.draw(batch, &self.gui.atlas);
        }
    }
}
