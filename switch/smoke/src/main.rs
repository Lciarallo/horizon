//! Smoke test for Rust std on the Switch: TLS, threads, locks, channels, HashMap,
//! the SD card, time, unwinding. Prints to the screen and to
//! sdmc:/switch/horizon/smoke.txt; + quits.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock, mpsc};
use std::time::{Duration, Instant, SystemTime};

thread_local! {
    static COUNTER: Cell<u64> = const { Cell::new(0) };
    static LAZY: String = format!("lazy-{}", 7);
}

fn check(report: &mut String, name: &str, f: impl FnOnce() -> Result<String, String> + std::panic::UnwindSafe) {
    let line = match std::panic::catch_unwind(f) {
        Ok(Ok(detail)) => format!("[ OK ] {name}: {detail}"),
        Ok(Err(e)) => format!("[FAIL] {name}: {e}"),
        Err(_) => format!("[PANIC] {name}"),
    };
    println!("{line}");
    horizon_switch::console::update();
    let _ = writeln!(report, "{line}");
}

fn main() {
    horizon_switch::console::init();
    println!("Horizon Switch smoke test\n");
    let mut report = String::new();

    check(&mut report, "tls (main)", || {
        COUNTER.with(|c| c.set(c.get() + 41));
        COUNTER.with(|c| c.set(c.get() + 1));
        let v = COUNTER.with(Cell::get);
        let lazy = LAZY.with(|s| s.clone());
        if v == 42 && lazy == "lazy-7" { Ok(format!("{v} {lazy}")) } else { Err(format!("{v} {lazy}")) }
    });

    check(&mut report, "threads + tls", || {
        let total = Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..6)
            .map(|i| {
                let total = total.clone();
                std::thread::Builder::new()
                    .name(format!("worker-{i}"))
                    .stack_size(256 * 1024)
                    .spawn(move || {
                        for _ in 0..1000 {
                            COUNTER.with(|c| c.set(c.get() + 1));
                        }
                        let mine = COUNTER.with(Cell::get) as usize;
                        total.fetch_add(mine, Ordering::SeqCst);
                        mine
                    })
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<_, _>>()?;
        let each: Vec<usize> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let sum = total.load(Ordering::SeqCst);
        if sum == 6000 && each.iter().all(|&n| n == 1000) { Ok(format!("sum {sum}")) } else { Err(format!("{each:?}")) }
    });

    check(&mut report, "mutex/condvar/rwlock", || {
        let pair = Arc::new((Mutex::new(0u32), Condvar::new()));
        let p2 = pair.clone();
        let t = std::thread::spawn(move || {
            let (m, cv) = &*p2;
            *m.lock().unwrap() = 99;
            cv.notify_one();
        });
        let (m, cv) = &*pair;
        let mut g = m.lock().unwrap();
        while *g != 99 {
            let (ng, _) = cv.wait_timeout(g, Duration::from_secs(5)).unwrap();
            g = ng;
        }
        drop(g);
        t.join().unwrap();
        let rw = RwLock::new(5);
        let r = *rw.read().unwrap();
        *rw.write().unwrap() += r;
        Ok(format!("cv ok, rw {}", *rw.read().unwrap()))
    });

    check(&mut report, "channel", || {
        let (tx, rx) = mpsc::channel();
        for i in 0..4u64 {
            let tx = tx.clone();
            std::thread::spawn(move || tx.send(i * i).unwrap());
        }
        drop(tx);
        let s: u64 = rx.iter().sum();
        if s == 14 { Ok(s.to_string()) } else { Err(s.to_string()) }
    });

    check(&mut report, "hashmap", || {
        let mut m = HashMap::new();
        for i in 0..10_000u32 {
            m.insert(i, i * 2);
        }
        Ok(format!("{} entries, m[1234]={}", m.len(), m[&1234]))
    });

    check(&mut report, "sd card", || {
        let dir = std::path::Path::new("sdmc:/switch/horizon/smoke-dir");
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("a.txt"), b"hello switch").map_err(|e| e.to_string())?;
        let back = std::fs::read_to_string(dir.join("a.txt")).map_err(|e| e.to_string())?;
        let names: Vec<String> = std::fs::read_dir(dir)
            .map_err(|e| e.to_string())?
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        let meta = std::fs::metadata(dir.join("a.txt")).map_err(|e| e.to_string())?;
        Ok(format!("{back:?} {names:?} len {}", meta.len()))
    });

    check(&mut report, "time", || {
        let t0 = Instant::now();
        std::thread::sleep(Duration::from_millis(50));
        let dt = t0.elapsed();
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_err(|e| e.to_string())?;
        Ok(format!("slept {dt:?}, unix {}", now.as_secs()))
    });

    check(&mut report, "unwind", || {
        let r = std::panic::catch_unwind(|| panic!("expected"));
        if r.is_err() { Ok("caught".into()) } else { Err("no panic".into()) }
    });

    check(&mut report, "memory", || {
        let (total, used) = horizon_switch::memory();
        Ok(format!("{} MiB total, {} MiB used, applet {}", total >> 20, used >> 20, horizon_switch::is_applet()))
    });

    check(&mut report, "parallelism", || {
        std::thread::available_parallelism().map(|n| n.to_string()).map_err(|e| e.to_string())
    });

    check(&mut report, "dns", || {
        use std::net::ToSocketAddrs;
        let addrs: Vec<_> = ("example.com", 443).to_socket_addrs().map_err(|e| e.to_string())?.collect();
        Ok(format!("{addrs:?}"))
    });

    // TCP: the plain blocking connect, and std's connect_timeout (non-blocking connect, poll,
    // SO_ERROR), which ureq uses when a connect timeout is set.
    check(&mut report, "tcp connect", || {
        use std::net::{TcpStream, ToSocketAddrs};
        let addr = ("example.com", 443).to_socket_addrs().map_err(|e| e.to_string())?
            .find(|a| a.is_ipv4()).ok_or("no IPv4 address")?;
        TcpStream::connect(addr).map(|s| format!("{addr} ok, local {:?}", s.local_addr())).map_err(|e| format!("{addr}: {e} ({e:?})"))
    });
    check(&mut report, "tcp connect_timeout", || {
        use std::net::{TcpStream, ToSocketAddrs};
        let addr = ("example.com", 443).to_socket_addrs().map_err(|e| e.to_string())?
            .find(|a| a.is_ipv4()).ok_or("no IPv4 address")?;
        TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(10))
            .map(|_| format!("{addr} ok"))
            .map_err(|e| format!("{addr}: {e} ({e:?})"))
    });

    let _ = std::fs::write("sdmc:/switch/horizon/smoke.txt", &report);
    println!("\nWritten to sdmc:/switch/horizon/smoke.txt. Press + to quit.");
    horizon_switch::input::init();
    while horizon_switch::main_loop() {
        let s = horizon_switch::input::poll();
        if s.down & horizon_switch::input::button::PLUS != 0 {
            break;
        }
        horizon_switch::console::update();
    }
    horizon_switch::console::exit();
}
