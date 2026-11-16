//! The Nintendo Switch's controller (docs/switch.md): player 1's Joy-Cons, handheld or
//! apart, or a Pro Controller, read through libnx once per poll. No gilrs (it has no Switch
//! backend), so this stands where `hardware.rs` does in a `controllers` build. Splitscreen's
//! router sees no extra pads.
use super::{super::Game, Pads};
use horizon_client::controller_ui::Device;
use horizon_client::gamepad::{Button, GlyphStyle, PadState, Pulse, Rumble};
use horizon_switch::input::button as nx;
use serde_json::{Value, json};

/// libnx's buttons by place: B is the bottom face button (South), A the right one (East).
const BUTTONS: [(u64, Button); 14] = [
    (nx::B, Button::South),
    (nx::A, Button::East),
    (nx::Y, Button::West),
    (nx::X, Button::North),
    (nx::L | nx::LEFT_SL | nx::RIGHT_SL, Button::LeftBumper),
    (nx::R | nx::LEFT_SR | nx::RIGHT_SR, Button::RightBumper),
    (nx::MINUS, Button::Select),
    (nx::PLUS, Button::Start),
    (nx::STICK_L, Button::LeftStick),
    (nx::STICK_R, Button::RightStick),
    (nx::UP, Button::DPadUp),
    (nx::DOWN, Button::DPadDown),
    (nx::LEFT, Button::DPadLeft),
    (nx::RIGHT, Button::DPadRight),
];

fn stick(x: i32, y: i32) -> [f32; 2] {
    let axis = |v: i32| (v as f32 / 32767.0).clamp(-1.0, 1.0);
    [axis(x), axis(y)]
}

/// The pad's state as the game reads pads: buttons by place, sticks right and up positive,
/// ZL and ZR as fully pulled or released triggers (they are digital).
fn state_of(input: &horizon_switch::input::State) -> PadState {
    let mut state = PadState::default();
    for (bits, button) in BUTTONS {
        state.set(button, input.held & bits != 0);
    }
    state.left = stick(input.lx, input.ly);
    state.right = stick(input.rx, input.ry);
    let pulled = |bit: u64| if input.held & bit != 0 { 1.0 } else { 0.0 };
    state.triggers = [pulled(nx::ZL), pulled(nx::ZR)];
    state
}

/// The controller's connection and style (`HidNpadStyleTag` bits: 1 Pro Controller,
/// 2 handheld, 4 Joy-Con pair, 8 left alone, 16 right alone) when they change.
fn log_connection(input: &horizon_switch::input::State) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST: AtomicU64 = AtomicU64::new(u64::MAX);
    let now = (u64::from(input.connected) << 32) | u64::from(input.style_set);
    if LAST.swap(now, Ordering::Relaxed) != now {
        log::info!(
            "Horizon: Switch controller {} (styles {:#x})",
            if input.connected != 0 { "connected" } else { "not connected" },
            input.style_set
        );
    }
}

fn device() -> Device {
    Device {
        name: "Nintendo Switch controller".to_owned(),
        vendor: Some(0x057e),
        product: Some(0x2009),
    }
}

impl Pads {
    pub(super) fn read(&mut self) -> PadState {
        let input = horizon_switch::input::poll();
        horizon_switch::input::note_game_read();
        log_connection(&input);
        self.style = GlyphStyle::Nintendo;
        if input.connected == 0 {
            self.device = None;
            return PadState::default();
        }
        self.device = Some(device());
        let state = state_of(&input);
        // The furthest each stick leant since the last poll: libnx reports the latest
        // sample only, which is that.
        self.peaks = [state.left, state.right];
        state
    }
    pub(in crate::game) fn release_hardware(&mut self) {}
}

pub(in crate::game) type PadId = usize;
#[derive(Default)]
pub(in crate::game) struct PadRouter {}
impl PadRouter {
    pub(in crate::game) fn adopt(&mut self, _: &mut Pads) {}
    pub(in crate::game) fn hand_back(&mut self, _: &mut Pads) {}
    pub(in crate::game) fn events(&mut self) -> Vec<super::PadEvent> {
        Vec::new()
    }
    pub(in crate::game) fn device(&self, _: PadId) -> Option<Device> {
        None
    }
    pub(in crate::game) fn lend_output(&mut self, _: &mut Pads, _: PadId) {}
    pub(in crate::game) fn reclaim_output(&mut self, _: &mut Pads) {}
    pub(in crate::game) fn play_rumbles(&mut self, _: &mut Pads, _: Option<PadId>, _: bool, _: bool) {}
    pub(in crate::game) fn report_status(&self, _: &mut Pads, _: Option<PadId>) {}
    pub(in crate::game) fn connected(&self, _: PadId) -> bool {
        false
    }
    pub(in crate::game) fn uuid(&self, _: PadId) -> [u8; 16] {
        [0; 16]
    }
    pub(in crate::game) fn state(&self, _: PadId) -> PadState {
        PadState::default()
    }
    pub(in crate::game) fn peaks(&self, _: PadId) -> [[f32; 2]; 2] {
        [[0.0; 2]; 2]
    }
    pub(in crate::game) fn name(&self, _: PadId) -> String {
        String::new()
    }
    pub(in crate::game) fn style(&self, _: PadId) -> GlyphStyle {
        GlyphStyle::Nintendo
    }
}

impl Game<'_> {
    pub(super) fn pad_output_off(&mut self) {}
    pub(super) fn pad_output_frame(&mut self) {}
    pub(in crate::game) fn pad_close_output(&mut self) {}
    pub(super) fn pad_vibrate(&mut self, _: Rumble, _: Pulse) {}
    pub(super) fn pad_devices_status(&self) -> Value {
        json!({ "backend": "libnx", "controllers": [device().name] })
    }
}
