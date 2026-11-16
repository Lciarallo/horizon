//! Startup shared by the gameplay library and its executable.
use crate::{crash, game, logging};

/// Start the game. The executable supplies the allocator and retains the mod API marker.
pub fn main() {
    // The Nintendo Switch: the loader gives the main thread a small stack, where desktops give
    // 8 MiB; the game runs on a thread of its own with room for vanilla's deep recursions.
    #[cfg(target_os = "horizon")]
    {
        switch_startup();
        let game = std::thread::Builder::new()
            .name("Render thread".into())
            .stack_size(SWITCH_MAIN_STACK)
            .spawn(|| main_with(Default::default()));
        match game.map(std::thread::JoinHandle::join) {
            Ok(Ok(())) => {}
            Ok(Err(_)) => eprintln!("Horizon: the game thread panicked"),
            Err(e) => eprintln!("Horizon: could not start the game thread: {e}"),
        }
        horizon_switch::log_flush();
        return;
    }
    #[cfg(not(target_os = "horizon"))]
    main_with(Default::default());
}

/// The game thread's stack on the Switch.
#[cfg(target_os = "horizon")]
const SWITCH_MAIN_STACK: usize = 16 << 20;

/// Start the game with trait mods in dispatch order.
pub fn main_with(mods: crate::mod_api::Mods) {
    // First: a trait injection into anything below (logging, the crash handler) calls its mod.
    if crate::mod_api::MODS.set(mods).is_err() {
        log::warn!("Trait mods already installed; ignoring the second list");
    }
    horizon_threads::configure_allocator();
    // The platform's features the dedicated server loads from server.toml, through the engine's hooks.
    horizon_chat_filter::install();
    crate::horizon_look::give();
    // OptiFine's and Polytone's pack formats (platform, docs/layers.md), before any pack loads.
    horizon_client::pack_formats::install();
    // horizon-extras (docs/forking.md): without the feature, no Horizon Settings or Mods page.
    if !cfg!(feature = "horizon-extras") {
        horizon_client::options_ui::HORIZON_EXTRAS
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
    horizon_client::telemetry::mark_start();
    // Opt-in telemetry keeps each crash report (docs/telemetry.md), whatever the mode.
    crash::on_report(horizon_client::telemetry::keep_crash);
    // Crates above the engine registered their hooks before this (horizon-official's `main`),
    // then the Horizon parts still in this crate; none registers after this point.
    game::player_hooks::register();
    horizon_hooks::engine::seal();
    // The text crates ship (`horizon_hooks::lang`), loaded with the game's own languages.
    horizon_client::lang::install_crate_files(horizon_hooks::lang::engine::all());
    horizon_client::spans::startup::init(std::time::Instant::now());
    let args: Vec<String> = std::env::args().skip(1).collect();
    let game_args = args.iter().take_while(|a| *a != "--dedicated");
    let help = game_args
        .clone()
        .any(|a| a == "--help" || a == "-h" || a == "--launch-api");
    // --home before the log opens in it.
    if let Some(dir) = game_args.clone().skip_while(|a| *a != "--home").nth(1) {
        logging::set_game_home(std::path::absolute(dir).unwrap_or_else(|_| dir.into()));
    }
    // The dedicated server logs in its own folder (--game-dir's), as vanilla's does; the game in its home.
    let (home, side, thread) = if args.iter().any(|a| a == "--dedicated") {
        let folder = game_args
            .clone()
            .skip_while(|a| *a != "--game-dir" && *a != "--gameDir")
            .nth(1)
            .and_then(|dir| std::path::absolute(dir).ok())
            .unwrap_or_else(|| ".".into());
        (folder, "server", "Server thread")
    } else {
        // --player-dir too (a game's, not a server's): a player of a shared game folder keeps
        // its log and crash reports in its own; crash telemetry stays with the home's consent.
        if let Some(dir) = game_args
            .clone()
            .skip_while(|a| *a != "--player-dir")
            .nth(1)
        {
            logging::set_player_dir(std::path::absolute(dir).unwrap_or_else(|_| dir.into()));
        }
        (logging::game_home(), "client", "Render thread")
    };
    // Before the log opens, so a mod's injection hears its first lines.
    logging::set_line_hook(crate::native_client_hooks::native_log_line);
    if !help {
        logging::init(
            logging::player_dir_override().unwrap_or(home.as_path()),
            thread,
        );
        // The log's first line names the build, so a bug report that carries the log says which one it was.
        log::info!("Horizon {}", crash::version());
        crash::install(home.clone(), side);
    }
    let renderers = crate::mod_api::mods()
        .iter()
        .filter_map(|m| m.render())
        .collect();
    if crate::gfx::native_render::install(renderers).is_err() {
        log::warn!("Trait renderers already installed; keeping the first list");
    }
    if horizon_world::native_load_hooks::install(
        horizon_world::native_load_hooks::NativeLoadHooks {
            loot_table_load: |id, table| {
                crate::mod_api::dispatch_loot_table_load(crate::mod_api::mods(), id, table);
            },
            modify_default_components: |item, components| {
                crate::mod_api::dispatch_modify_default_components(
                    crate::mod_api::mods(),
                    item,
                    components,
                );
            },
            loot_function: |kind, data, ctx, stack| {
                crate::mod_api::dispatch_loot_function(
                    crate::mod_api::mods(),
                    kind,
                    data,
                    ctx,
                    stack,
                );
            },
            loot_condition: |kind, data, ctx, holds| {
                crate::mod_api::dispatch_loot_condition(
                    crate::mod_api::mods(),
                    kind,
                    data,
                    ctx,
                    holds,
                );
            },
        },
    )
    .is_err()
    {
        log::warn!("Native data-load hooks already installed; keeping the first callbacks");
    }
    if horizon_world::worldgen::native_generate::install(
        horizon_world::worldgen::native_generate::NativeGenerateHooks {
            chunk_generate: |dimension, chunk, seed, flat, chunk_out, generate| {
                crate::mod_api::dispatch_chunk_generate(
                    crate::mod_api::mods(),
                    dimension,
                    chunk,
                    seed,
                    flat,
                    chunk_out,
                    generate,
                );
            },
            lod_column: |dimension, column, seed, flat, column_out| {
                crate::mod_api::dispatch_lod_column(
                    crate::mod_api::mods(),
                    dimension,
                    column,
                    seed,
                    flat,
                    column_out,
                );
            },
        },
    )
    .is_err()
    {
        log::warn!("Native generation hooks already installed; keeping the first callbacks");
    }
    if crate::native_client_hooks::install(crate::native_client_hooks::NativeClientHooks {
        menu_slot_rules: |menu, slot, stack, may_place, max_stack| {
            crate::mod_api::dispatch_menu_slot_rules(
                crate::mod_api::mods(),
                menu,
                slot,
                stack,
                may_place,
                max_stack,
            );
        },
        player_chat: |player, name, message, replies, cancel| {
            crate::mod_api::dispatch_player_chat(
                crate::mod_api::mods(),
                player,
                name,
                message,
                replies,
                cancel,
            );
        },
        player_chat_v2: |player, name, level, message, replies, cancel| {
            crate::mod_api::dispatch_player_chat_v2(
                crate::mod_api::mods(),
                player,
                name,
                level,
                message,
                replies,
                cancel,
            );
        },
        chat_decorate: |player, name, message, decorated| {
            crate::mod_api::dispatch_chat_decorate(
                crate::mod_api::mods(),
                player,
                name,
                message,
                decorated,
            );
        },
        sound_play: |sound, volume, pitch, cancel| {
            crate::mod_api::dispatch_sound_play(
                crate::mod_api::mods(),
                sound,
                volume,
                pitch,
                cancel,
            );
        },
        living_heal: |entity, amount, cancel| {
            crate::mod_api::dispatch_living_heal(crate::mod_api::mods(), entity, amount, cancel);
        },
        living_breathe: |entity, kind, can_breathe, consume, refill| {
            crate::mod_api::dispatch_living_breathe(
                crate::mod_api::mods(),
                entity,
                kind,
                can_breathe,
                consume,
                refill,
            );
        },
        living_drown: |entity, kind, is_drowning, damage_amount, bubble_count, cancel| {
            crate::mod_api::dispatch_living_drown(
                crate::mod_api::mods(),
                entity,
                kind,
                is_drowning,
                damage_amount,
                bubble_count,
                cancel,
            );
        },
        living_fall: |entity, kind, distance, multiplier, cancel| {
            crate::mod_api::dispatch_living_fall(
                crate::mod_api::mods(),
                entity,
                kind,
                distance,
                multiplier,
                cancel,
            );
        },
        living_jump: |entity, kind| {
            crate::mod_api::dispatch_living_jump(crate::mod_api::mods(), entity, kind);
        },
        living_death_message: |entity, kind, message| {
            crate::mod_api::dispatch_living_death_message(
                crate::mod_api::mods(),
                entity,
                kind,
                message,
            );
        },
        register_goals: |mob| {
            crate::mod_api::dispatch_register_goals(crate::mod_api::mods(), mob);
        },
        player_pre_login: |player, name, address, defer| {
            crate::mod_api::dispatch_player_pre_login(
                crate::mod_api::mods(),
                player,
                name,
                address,
                defer,
            );
        },
        player_profile: |player, name, properties| {
            crate::mod_api::dispatch_player_profile(
                crate::mod_api::mods(),
                player,
                name,
                properties,
            );
        },
        ingredient: |kind, data, items| {
            crate::mod_api::dispatch_ingredient(crate::mod_api::mods(), kind, data, items);
        },
        ingredient_test: |kind, data, stack, accept| {
            crate::mod_api::dispatch_ingredient_test(
                crate::mod_api::mods(),
                kind,
                data,
                stack,
                accept,
            );
        },
        log_line: |level, thread, message, line| {
            crate::mod_api::dispatch_log_line(crate::mod_api::mods(), level, thread, message, line);
        },
        handshake: |address, host, intent, cancel, reason| {
            crate::mod_api::dispatch_handshake(
                crate::mod_api::mods(),
                address,
                host,
                intent,
                reason,
                cancel,
            );
        },
    })
    .is_err()
    {
        log::warn!("Native client hooks already installed; keeping the first callbacks");
    }
    if horizon_client::sound_engine::install_playback_hook(
        horizon_client::sound_playback::native_sound_play,
    )
    .is_err()
    {
        log::warn!("Native sound hook already installed; keeping the first callback");
    }
    raise_open_file_limit();
    if let Err(error) = game::run_with() {
        // A bad command line is the caller's mistake, not a crash: no report.
        let usage = error.is::<game::UsageError>();
        log::error!("Horizon: {error}");
        logging::flush();
        // The report's path is the last line on stderr (crash::SAVED_LINE).
        if !help && !usage {
            crash::report_error(&home, side, &*error);
        }
        // Started from Explorer, the console window closes with the game:
        // keep the message on screen until it is read.
        #[cfg(windows)]
        if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            eprintln!("Press Enter to close.");
            let _ = std::io::stdin().read_line(&mut String::new());
        }
        std::process::exit(if usage {
            game::exit_code::USAGE
        } else {
            game::exit_code::ERROR
        });
    }
    horizon_hooks::engine::shutdown();
    logging::flush();
}

/// The Nintendo Switch (docs/switch.md): the game's folder on the SD card is its home and its
/// working directory (saves, options, logs), and what would reach a terminal goes to a file
/// there (or to `nxlink -s`).
#[cfg(target_os = "horizon")]
fn switch_startup() {
    let home = std::path::Path::new(horizon_switch::HOME);
    let _ = std::fs::create_dir_all(home);
    horizon_switch::log_to_file(&format!("{}/horizon-output.log", horizon_switch::HOME));
    if let Err(e) = std::env::set_current_dir(home) {
        eprintln!("Horizon: game folder {}: {e}", home.display());
    }
    logging::set_game_home(home.to_path_buf());
    horizon_switch::boost_for_loading();
    let (total, used) = horizon_switch::memory();
    println!(
        "Horizon on Nintendo Switch: {} MiB of memory, {} MiB in use{}",
        total >> 20,
        used >> 20,
        if horizon_switch::is_applet() { " (applet mode: start a game holding R for all of it)" } else { "" }
    );
    // Every 15 s, the memory in use: a stalled start tells working from stuck.
    let _ = std::thread::Builder::new()
        .name("Switch watchdog".into())
        .stack_size(64 << 10)
        .spawn(|| {
            let started = std::time::Instant::now();
            // After the log has opened.
            std::thread::sleep(std::time::Duration::from_secs(8));
            malloc_bench();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(15));
                let (total, _) = horizon_switch::memory();
                log::info!(
                    "Horizon: {} s up, heap {} of {} MiB",
                    started.elapsed().as_secs(),
                    horizon_switch::heap_in_use() >> 20,
                    total >> 20
                );
            }
        });
    // A panic's message reaches the log before the program ends.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        horizon_switch::log_flush();
    }));
}

/// How fast the heap hands out small blocks, alone and with three threads at once: the
/// Switch's heap is newlib's, behind one lock (docs/switch.md).
#[cfg(target_os = "horizon")]
fn malloc_bench() {
    fn churn(n: usize) -> usize {
        let mut keep: Vec<Vec<u8>> = Vec::with_capacity(64);
        let mut sum = 0;
        for i in 0..n {
            let v = vec![i as u8; 16 + (i * 37) % 240];
            sum += v.len();
            if keep.len() < 64 {
                keep.push(v);
            } else {
                keep[i % 64] = v;
            }
        }
        sum + keep.len()
    }
    const N: usize = 200_000;
    let t = std::time::Instant::now();
    std::hint::black_box(churn(N));
    let one = t.elapsed();
    let t = std::time::Instant::now();
    let threads: Vec<_> = (0..3).map(|_| std::thread::spawn(|| std::hint::black_box(churn(N)))).collect();
    for thread in threads {
        let _ = thread.join();
    }
    let three = t.elapsed();
    log::info!(
        "Horizon: heap {} allocations: 1 thread {:.0} ms, 3 threads at once {:.0} ms",
        N,
        one.as_secs_f64() * 1000.0,
        three.as_secs_f64() * 1000.0
    );
}

/// The JVM raises its open-file limit to the hard limit as it starts
/// (`-XX:+MaxFDLimit`, on by default), so a vanilla server may hold as many
/// sockets and region files as the host allows, not the soft default (often
/// 1024 in a container). The same here; macOS caps it at `OPEN_MAX`, as the
/// JVM does there.
fn raise_open_file_limit() {
    #[cfg(all(unix, not(target_os = "horizon")))]
    {
        use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
        let limit = getrlimit(Resource::Nofile);
        let hard = if cfg!(target_os = "macos") {
            Some(limit.maximum.map_or(10_240, |h| h.min(10_240)))
        } else {
            limit.maximum
        };
        let raised = match (limit.current, hard) {
            (Some(soft), Some(hard)) => hard > soft,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if raised {
            let to = Rlimit {
                current: hard,
                maximum: limit.maximum,
            };
            match setrlimit(Resource::Nofile, to) {
                Ok(()) => log::info!(
                    "Open file limit raised from {:?} to {hard:?}",
                    limit.current
                ),
                Err(e) => log::warn!("Open file limit stays at {:?}: {e}", limit.current),
            }
        }
    }
}
