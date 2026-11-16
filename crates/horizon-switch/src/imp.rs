use std::ffi::{CStr, CString, c_char, c_void};

mod sys {
    use super::*;
    unsafe extern "C" {
        pub fn horizon_switch_log_to_file(path: *const c_char);
        pub fn horizon_switch_log_flush();
        pub fn horizon_switch_nxlink() -> bool;
        pub fn horizon_switch_is_applet() -> bool;
        pub fn horizon_switch_memory(total: *mut u64, used: *mut u64);
        pub fn horizon_switch_sd_space(free: *mut u64, total: *mut u64) -> bool;
        pub fn horizon_switch_heap_in_use() -> u64;
        pub fn horizon_switch_thread_enter();
        pub fn horizon_switch_input_init();
        pub fn horizon_switch_input_poll(out: *mut super::input::State);
        pub fn horizon_switch_touch_poll(out: *mut super::input::State);
        pub fn horizon_switch_display_size(w: *mut u32, h: *mut u32);
        pub fn horizon_switch_main_loop() -> bool;
        pub fn horizon_switch_default_window() -> *mut c_void;
        pub fn horizon_switch_focused() -> bool;
        pub fn horizon_switch_console_init();
        pub fn horizon_switch_console_update();
        pub fn horizon_switch_console_exit();
        pub fn horizon_switch_boost(on: bool);
        pub fn horizon_switch_keyboard(guide: *const c_char, initial: *const c_char, out: *mut c_char, len: usize) -> bool;
    }
}

/// getrandom 0.2's source on the Switch (0.3 and 0.4 call the shim's `getrandom`).
fn fill_random(dest: &mut [u8]) -> Result<(), getrandom02::Error> {
    unsafe extern "C" {
        fn getrandom(buf: *mut c_void, len: usize, flags: u32) -> isize;
    }
    unsafe { getrandom(dest.as_mut_ptr().cast(), dest.len(), 0) };
    Ok(())
}
getrandom02::register_custom_getrandom!(fill_random);

/// getrandom 0.3's and 0.4's custom backend (`--cfg getrandom_backend="custom"`, set by
/// switch/inner-build.sh). Both declare this symbol returning `Result<(), Error>`, and both
/// `Error`s are a `NonZeroU32`, so one definition serves the two.
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(dest: *mut u8, len: usize) -> Result<(), std::num::NonZeroU32> {
    unsafe extern "C" {
        fn getrandom(buf: *mut c_void, len: usize, flags: u32) -> isize;
    }
    unsafe { getrandom(dest.cast(), len, 0) };
    Ok(())
}

/// Where the game's files live on the SD card (`sdmc:/switch/horizon`). Written without the
/// device: Rust's paths take `sdmc:/...` for a relative path, and libnx reads a path that
/// starts with `/` on the current device, the SD card.
pub const HOME: &str = "/switch/horizon";

/// Sends stdout and stderr to `path` on the SD card, unless the program was started
/// by `nxlink -s`, which already receives them over the network.
pub fn log_to_file(path: &str) {
    let path = CString::new(path).expect("log path");
    unsafe { sys::horizon_switch_log_to_file(path.as_ptr()) }
}

pub fn log_flush() {
    unsafe { sys::horizon_switch_log_flush() }
}

/// Started by `nxlink`, with stdout and stderr going back to the computer.
pub fn nxlink() -> bool {
    unsafe { sys::horizon_switch_nxlink() }
}

/// Started as an applet (from the album or the Homebrew Menu without a game): only a
/// few hundred MB of memory. A title override (hold R while starting a game) has it all.
pub fn is_applet() -> bool {
    unsafe { sys::horizon_switch_is_applet() }
}

/// (total, used) memory of the process in bytes.
pub fn memory() -> (u64, u64) {
    let (mut total, mut used) = (0, 0);
    unsafe { sys::horizon_switch_memory(&mut total, &mut used) };
    (total, used)
}

/// Bytes allocated with malloc and not freed (the game's heap in use).
pub fn heap_in_use() -> u64 {
    unsafe { sys::horizon_switch_heap_in_use() }
}

/// The SD card's (free, total) bytes.
pub fn sd_space() -> Option<(u64, u64)> {
    let (mut free, mut total) = (0, 0);
    unsafe { sys::horizon_switch_sd_space(&mut free, &mut total) }.then_some((free, total))
}

/// For a thread created by libnx directly (not `std::thread`) before it runs Rust code.
pub fn thread_enter() {
    unsafe { sys::horizon_switch_thread_enter() }
}

/// False once the system asks the program to quit (HOME menu close).
pub fn main_loop() -> bool {
    unsafe { sys::horizon_switch_main_loop() }
}

pub fn focused() -> bool {
    unsafe { sys::horizon_switch_focused() }
}

/// The screen size: 1280x720 handheld, 1920x1080 docked.
pub fn display_size() -> (u32, u32) {
    let (mut w, mut h) = (1280, 720);
    unsafe { sys::horizon_switch_display_size(&mut w, &mut h) };
    (w, h)
}

/// libnx's default `NWindow*`, the native window EGL renders to.
pub fn default_window() -> *mut c_void {
    unsafe { sys::horizon_switch_default_window() }
}

/// CPU boost (1785 MHz) for loading.
pub fn boost(on: bool) {
    unsafe { sys::horizon_switch_boost(on) }
}

static BOOSTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static WORLD_FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// CPU boost while the game loads or opens a world (`ApmCpuBoostMode_FastLoad`: the CPU at
/// 1785 MHz, the GPU slowed meanwhile), until [`world_frame`] sees the world loaded.
pub fn boost_for_loading() {
    use std::sync::atomic::Ordering;
    WORLD_FRAMES.store(0, Ordering::Relaxed);
    if !BOOSTED.swap(true, Ordering::Relaxed) {
        boost(true);
    }
}

/// A frame drawn in a world, with `pending` chunks still to load, generate or upload: the
/// boost ends once nothing waits, or after about a minute of play (the GPU needs its clock).
pub fn world_frame(pending: usize) {
    use std::sync::atomic::Ordering;
    if !BOOSTED.load(Ordering::Relaxed) {
        return;
    }
    let frames = WORLD_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
    if (pending == 0 && frames > 30) || frames > 1800 {
        BOOSTED.store(false, Ordering::Relaxed);
        boost(false);
    }
}

/// The system keyboard. `None` when cancelled.
pub fn keyboard(guide: &str, initial: &str) -> Option<String> {
    let guide = CString::new(guide).ok()?;
    let initial = CString::new(initial).ok()?;
    let mut buf = vec![0 as c_char; 512];
    let ok = unsafe { sys::horizon_switch_keyboard(guide.as_ptr(), initial.as_ptr(), buf.as_mut_ptr(), buf.len()) };
    if !ok {
        return None;
    }
    let text = unsafe { CStr::from_ptr(buf.as_ptr()) };
    Some(text.to_string_lossy().into_owned())
}

/// Sound out through libnx's audout (cpal's Nintendo Switch host): 16-bit stereo.
pub mod audio {
    unsafe extern "C" {
        fn horizon_switch_audio_open(frames: u32, rate: *mut u32, channels: *mut u32) -> bool;
        fn horizon_switch_audio_frames() -> u32;
        fn horizon_switch_audio_write(samples: *const i16) -> bool;
    }

    /// Opens the output (once; later calls report it): `frames` per buffer. (rate, channels).
    pub fn open(frames: u32) -> Option<(u32, u32)> {
        let (mut rate, mut channels) = (0, 0);
        unsafe { horizon_switch_audio_open(frames, &mut rate, &mut channels) }.then_some((rate, channels))
    }

    /// Frames per buffer of the open output (0 before `open`).
    pub fn frames() -> u32 {
        unsafe { horizon_switch_audio_frames() }
    }

    /// Plays one buffer: `frames() * 2` interleaved samples. Blocks while the queue is full.
    pub fn write(samples: &[i16]) -> bool {
        assert_eq!(samples.len(), frames() as usize * 2, "one buffer of stereo frames");
        unsafe { horizon_switch_audio_write(samples.as_ptr()) }
    }
}

/// libnx's text console on the framebuffer (the smoke test; the game itself uses EGL).
/// 80 columns by 45 rows, ANSI escapes.
pub mod console {
    pub fn init() {
        unsafe { super::sys::horizon_switch_console_init() }
    }
    pub fn update() {
        unsafe { super::sys::horizon_switch_console_update() }
    }
    pub fn exit() {
        unsafe { super::sys::horizon_switch_console_exit() }
    }
}

/// Player 1's controller (handheld, Joy-Con pair, Pro Controller) and the touch screen.
pub mod input {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    fn epoch() -> Instant {
        static EPOCH: OnceLock<Instant> = OnceLock::new();
        *EPOCH.get_or_init(Instant::now)
    }
    static GAME_READ_AT: AtomicU64 = AtomicU64::new(0);
    static GAME_TOUCH_AT: AtomicU64 = AtomicU64::new(0);

    /// The game took the touch screen this frame (its on-screen controls, in a world).
    pub fn note_game_touch() {
        let ms = epoch().elapsed().as_millis() as u64;
        GAME_TOUCH_AT.store(ms.max(1), Ordering::Relaxed);
    }

    /// The game took the touch screen in the last quarter second: winit's backend then
    /// leaves it alone instead of making it the mouse.
    pub fn game_owns_touch() -> bool {
        let at = GAME_TOUCH_AT.load(Ordering::Relaxed);
        at != 0 && epoch().elapsed().saturating_sub(Duration::from_millis(at)) < Duration::from_millis(250)
    }

    /// The game read the pad for its own controller support this frame (in a world).
    pub fn note_game_read() {
        let ms = epoch().elapsed().as_millis() as u64;
        GAME_READ_AT.store(ms.max(1), Ordering::Relaxed);
    }

    /// The game read the pad itself in the last quarter second. While it does not (its
    /// startup menu takes only keys), winit's backend turns the pad into keys.
    pub fn game_reads_pad() -> bool {
        let at = GAME_READ_AT.load(Ordering::Relaxed);
        at != 0 && epoch().elapsed().saturating_sub(Duration::from_millis(at)) < Duration::from_millis(250)
    }

    /// `HidNpadButton` bits.
    pub mod button {
        pub const A: u64 = 1 << 0;
        pub const B: u64 = 1 << 1;
        pub const X: u64 = 1 << 2;
        pub const Y: u64 = 1 << 3;
        pub const STICK_L: u64 = 1 << 4;
        pub const STICK_R: u64 = 1 << 5;
        pub const L: u64 = 1 << 6;
        pub const R: u64 = 1 << 7;
        pub const ZL: u64 = 1 << 8;
        pub const ZR: u64 = 1 << 9;
        pub const PLUS: u64 = 1 << 10;
        pub const MINUS: u64 = 1 << 11;
        pub const LEFT: u64 = 1 << 12;
        pub const UP: u64 = 1 << 13;
        pub const RIGHT: u64 = 1 << 14;
        pub const DOWN: u64 = 1 << 15;
        /// The left stick pushed past libnx's threshold, as buttons.
        pub const STICK_L_LEFT: u64 = 1 << 16;
        pub const STICK_L_UP: u64 = 1 << 17;
        pub const STICK_L_RIGHT: u64 = 1 << 18;
        pub const STICK_L_DOWN: u64 = 1 << 19;
        /// Left Joy-Con held alone: its SL and SR.
        pub const LEFT_SL: u64 = 1 << 24;
        pub const LEFT_SR: u64 = 1 << 25;
        /// Right Joy-Con held alone: its SL and SR.
        pub const RIGHT_SL: u64 = 1 << 26;
        pub const RIGHT_SR: u64 = 1 << 27;
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct State {
        pub held: u64,
        pub down: u64,
        pub up: u64,
        pub lx: i32,
        pub ly: i32,
        pub rx: i32,
        pub ry: i32,
        pub style_set: u32,
        pub attributes: u32,
        pub connected: u32,
        pub touch_count: u32,
        pub touch_x: [i32; 4],
        pub touch_y: [i32; 4],
        pub touch_id: [u32; 4],
    }

    pub fn init() {
        unsafe { super::sys::horizon_switch_input_init() }
    }

    /// The controller and the touch screen. Once per frame, by one reader: the button
    /// edges (`down`, `up`) are since the previous call.
    pub fn poll() -> State {
        let mut state = State::default();
        unsafe { super::sys::horizon_switch_input_poll(&mut state) };
        state
    }

    /// The touch screen alone (only the `touch_*` fields are filled); leaves the
    /// controller's edges to `poll`.
    pub fn poll_touch() -> State {
        let mut state = State::default();
        unsafe { super::sys::horizon_switch_touch_poll(&mut state) };
        state
    }
}
