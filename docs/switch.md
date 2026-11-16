# Nintendo Switch (homebrew)

Horizon runs on a Nintendo Switch with custom firmware (Atmosphère) as a homebrew program,
`horizon.nro`, started from the Homebrew Menu. It is the same game as on a computer, built
for libnx: OpenGL through switch-mesa instead of Vulkan, the Joy-Cons through the game's own
controller support, the SD card for the game's folder. **Status: experimental.** It builds and
links; see [what is missing](#what-is-missing).

You need your own Minecraft: Java Edition 26.3 (a Microsoft account that owns it): the game's
resources are prepared from your installation on a computer, exactly as for the desktop game,
and copied to the SD card. Nothing of Mojang's is in the `.nro`.

## Build

Everything happens in a container (podman or docker): devkitPro's devkitA64 image, libnx,
switch-mesa and Rust nightly with `rust-src` (`packaging/switch/Dockerfile`).

```sh
switch/build.sh            # target/switch/horizon.nro (release: thin LTO)
switch/build.sh smoke      # target/switch/horizon-smoke.nro, a test of Rust's std on the console
HORIZON_SWITCH_PROFILE=fast switch/build.sh   # quicker to build, larger, slower
switch/build.sh check      # cargo check for the Switch
switch/build.sh shell      # a shell in the build container
```

The first build makes the image (about 3 GB) and compiles std for the Switch (`-Z build-std`).
Cargo does not see changes to std's sources in the image (`packaging/switch/patch-std.py`):
after one, remove `target/switch/aarch64-nintendo-switch-newlib/*/build/std` so std is built again.

## Install

1. Prepare the resources on a computer, as the [native client](native-client.md#run-from-a-checkout)
   says: install and start 26.3 once in the official launcher, then
   `horizon-prepare --prepare --complete --minecraft-dir ~/.minecraft --server-jar server.jar`.
   They land in `.local/horizon-client/26.3` (about 690 MB, 28,000 files). Without the server
   jar, `HORIZON_SERVER_FROM_CLIENT=1` uses the client jar for the server side, as Prism
   installs already do. The exporters need `javac` 25: the official launcher's own runtime has
   one (`--java-home ~/.minecraft/runtime/java-runtime-epsilon/linux/java-runtime-epsilon`).
2. Copy to the SD card (`sdmc:` below is the card's root):
   - `horizon.nro` to `sdmc:/switch/horizon/horizon.nro`
   - the folder `.local/horizon-client/26.3` to `sdmc:/switch/horizon/resources`
     (it holds `horizon-resources.json`)

   `switch/install-ftp.py SWITCH_IP:5000` does both over Wi-Fi with the FTP server of sphaira,
   ftpd or sys-ftpd (it skips files already there with the same size), and
   `switch/install-sd.sh MOUNT_POINT` with the card on the computer (hekate's USB mass storage
   or a card reader).
3. Start the Homebrew Menu **by holding R while starting a game** (title override). Started
   from the album, homebrew gets a few hundred MB of memory, far too little: the game needs
   more than 1.5 GB. Then pick Horizon.

The game's folder is `sdmc:/switch/horizon`: saves, `options.txt`, `logs/`, crash reports, and
`horizon-output.log` (what the program printed, panics included). With
`nxlink -s target/switch/horizon.nro` (Homebrew Menu, Y) the output comes to the computer instead.

## Controls

The game's own controller support (docs/controllers.md), with Nintendo's glyphs: buttons by
their place, so B (bottom) jumps and A (right) is the right-hand face button. ZL and ZR are
the triggers, + is Start (pause), − is Select. The touch screen is the mouse in menus; in a
world it brings up the on-screen controls of Horizon's Android work (`horizon_client_render::touch`,
`game/switch_touch.rs`): a joystick, jump, sneak, sprint, attack, use, inventory, chat, pause,
camera and swap hands, with a drag on the rest of the screen turning the view. They show once
the screen is touched and hide when a controller is used. The startup menu (title, world
creation, options) takes only keys, so there the controller is keys: the D-pad and left stick
are the arrows, A Enter, B Escape, X Tab, Y Space. A text box (chat, a world's name, a search) opens the system keyboard; its text is typed
into the box when it closes.

## Icon

`switch/tools/make-icon.py` draws the Homebrew Menu icon, a grass block on a sky, from the
player's own prepared resources (the textures are Mojang's, so the icon stays out of the
repository). It writes `.local/switch/icon.jpg`, which `switch/build.sh` uses when it is there;
otherwise the `.nro` carries `switch/nro/icon.jpg`.

## How it works

- **std on libnx.** The custom target `switch/aarch64-nintendo-switch-newlib.json` is
  `target_os = "horizon"`, `target_env = "newlib"`, unix family: Rust's std already has a
  `horizon` port (the 3DS's, on newlib), built with `-Z build-std`. The `libc` crate's
  definitions are the 3DS's, so `third_party/libc` corrects them for the Switch (16-bit
  `dev_t`/`uid_t`, BSD sockets, 16-bit `pollfd` events, devkitPro's pthread sizes and its
  condition variable initializer...). `switch/tools/abi_check` asserts at compile time that
  every size, offset and constant std and the crates use matches devkitA64's headers
  (`switch/tools/abi_dump.sh` reads them from GCC). std's own `libc` is the same copy
  (`packaging/switch/Dockerfile`).
- **Thread-locals.** libnx keeps the ELF thread pointer in its ThreadVars (read through
  `TPIDRRO_EL0`), which GCC reaches with `-mtp=soft`; LLVM only reads `TPIDR_EL0`. The startup
  shim (`crates/horizon-switch/shim/horizon_switch.c`) copies libnx's pointer into `TPIDR_EL0`
  on the main thread (`userAppInit`) and on every new thread (`pthread_create` is linked with
  `--wrap`), so Rust's native TLS works as compiled.
- **Detached threads.** libnx has no `pthread_detach` (newlib answers `ENOSYS`), so a thread
  std detached (a dropped `JoinHandle`) was never closed, and after a few minutes every spawn
  failed with `ENOMEM` and the game quit. The shim also wraps `pthread_detach` and
  `pthread_join`: a detached thread is joined by the next `pthread_create` or `pthread_detach`
  once it has finished. The audio output, the console's only one, is not probed for changes.
- **The shim** also supplies what newlib lacks (`getrandom` from libnx's csrng, `sysconf`,
  `posix_memalign`, `pread`/`pwrite`), opens the sockets service and sends output to nxlink or
  the SD card.
- **Window and input.** `third_party/winit` has a Switch backend
  (`src/platform_impl/horizon`): one 1280x720 window, libnx's `NWindow` handed to EGL as an
  Android window, the touch screen as the mouse, the HOME menu's focus and exit, and the system
  keyboard while the game allows text input. The controller is read by the game itself
  (`game/pad/switch_pad.rs`), since gilrs has no Switch backend.
- **Heap.** mimalloc, on primitives of its own (`third_party/libmimalloc-sys`:
  `src/prim/horizon`) that take its arenas from newlib's heap. newlib's malloc alone, one
  lock for every thread, took 38 µs an allocation on the console and made a world's first
  load take over ten minutes.
- **Sound.** cpal's Switch host, `audout` through the shim (`shim/audio.c`).
- **HTTPS.** ureq with rustls and webpki-roots' certificates (the Switch's own store is not
  reachable), ring for the cryptography (`third_party/ring` knows the Switch).
- **Graphics.** wgpu's OpenGL ES backend on switch-mesa's EGL: `third_party/wgpu-hal` links
  libEGL statically and takes the default display on the Switch. The renderer tries OpenGL only.
- **Crates that knew only the 3DS among `horizon` targets**: `third_party/mio` (pollfd,
  sockaddr_in), `third_party/ring` (its AArch64 assembly), getrandom (its custom backend,
  defined in `horizon-switch`). mimalloc has no libnx backend: `switch/stubs` stand in for it
  with newlib's heap on the Switch and are the real crates elsewhere. arboard, socket2, fs4 and
  rustix are left out on the Switch (`cfg(not(target_os = "horizon"))`).

## What is missing

- **Shaders are compiled at every start.** switch-mesa is built without its disk shader cache,
  so the first frames after starting (a black screen) take a couple of minutes. A deko3d
  backend would fix it. `horizon-output.log` and `logs/latest.log` on the card say how far a
  start got.
- Sound goes through cpal's Switch host (`third_party/cpal`: `src/host/horizon`, libnx's
  `audout`, 16-bit stereo at 48 kHz, about 85 ms queued); there is no microphone input.
- No mods (wasmtime), no Discord, no clipboard, no LAN keepalive tuning; one controller
  (player 1). The LAN server polls every 5 ms (mio has no waker without pipes).
- Render distance defaults to 6. The Switch has 4 GB shared with the GPU; far terrain and
  high settings will not fit.
