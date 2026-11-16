/*
 * Nintendo Switch startup shim for Horizon (libnx + newlib + Rust std).
 *
 * TLS: libnx keeps the ELF thread pointer in ThreadVars (read through tpidrro_el0,
 * `__aarch64_read_tp`), which is what devkitA64's GCC uses with -mtp=soft. LLVM can
 * only read TPIDR_EL0, a user register the kernel keeps per thread and libnx leaves
 * unused. So every thread copies libnx's pointer into TPIDR_EL0 before any Rust code
 * runs: the main thread in userAppInit (libnx calls it before main), every other
 * thread in the pthread_create wrapper (linked with --wrap=pthread_create).
 *
 * It also supplies the few POSIX calls newlib/libnx lack that std and its crates
 * use (getrandom, sysconf, posix_memalign, pread/pwrite, dlopen as always failing), and sends stdout/stderr to nxlink when
 * started by it, otherwise (horizon_switch_log_to_file) to a file on the SD card.
 */
#include <switch.h>
#include <errno.h>
#include <fcntl.h>
#include <malloc.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

extern void *__aarch64_read_tp(void);

static inline void horizon_sync_tp(void) {
    void *tp = __aarch64_read_tp();
    __asm__ volatile("msr tpidr_el0, %0" ::"r"(tp) : "memory");
}

extern char _start[];
/* Where the program's ELF starts in memory (its offset 0). */
static void *horizon_switch_code_base(void) { return _start; }

/* ---- threads ---- */

/* libnx has no pthread_detach (newlib's answers ENOSYS), so a detached thread was never
 * closed: its stack and kernel thread stayed until the process ran out of threads (ENOMEM
 * from every spawn after a few minutes, std dropping a JoinHandle being a detach). Every
 * thread therefore keeps a record until it is joined; a detached one, once it has finished, is
 * joined by a reaper thread of the shim's own. Joining it from pthread_create or pthread_detach
 * instead can deadlock: their caller may hold a lock (the log's) that the finished thread's
 * thread-local destructors still wait for. Linked with --wrap=pthread_detach and
 * --wrap=pthread_join. */
struct horizon_thread {
    void *(*fn)(void *);
    void *arg;
    pthread_t id;
    bool has_id;
    bool detached;
    _Atomic bool finished;
    struct horizon_thread *next;
};

static Mutex horizon_threads_lock;
static struct horizon_thread *horizon_threads;
static Thread horizon_reaper;
static bool horizon_reaper_started;
/* sdmc:/switch/horizon/no-reaper (read at start): detached threads are left unjoined, as libnx
 * leaves them, to tell the reaper's effects from the game's. */
static bool horizon_reaper_off;
static unsigned long horizon_threads_created, horizon_threads_joined, horizon_threads_reaped;

int __real_pthread_create(pthread_t *thread, const pthread_attr_t *attr, void *(*fn)(void *), void *arg);
int __real_pthread_join(pthread_t thread, void **retval);

static void *horizon_thread_entry(void *p) {
    horizon_sync_tp();
    struct horizon_thread *t = p;
    void *ret = t->fn(t->arg);
    __atomic_store_n(&t->finished, true, __ATOMIC_RELEASE);
    return ret;
}

/* Takes the record of `id` out of the list (the lock held). */
static struct horizon_thread *horizon_thread_take(pthread_t id) {
    for (struct horizon_thread **at = &horizon_threads; *at; at = &(*at)->next) {
        if ((*at)->has_id && pthread_equal((*at)->id, id)) {
            struct horizon_thread *t = *at;
            *at = t->next;
            return t;
        }
    }
    return NULL;
}

/* Joins the detached threads that have finished. Holds no lock while it waits. */
static void horizon_threads_reap(void) {
    struct horizon_thread *done = NULL;
    mutexLock(&horizon_threads_lock);
    for (struct horizon_thread **at = &horizon_threads; *at;) {
        struct horizon_thread *t = *at;
        if (t->detached && __atomic_load_n(&t->finished, __ATOMIC_ACQUIRE)) {
            *at = t->next;
            t->next = done;
            done = t;
        } else {
            at = &t->next;
        }
    }
    mutexUnlock(&horizon_threads_lock);
    while (done) {
        struct horizon_thread *next = done->next;
        __real_pthread_join(done->id, NULL);
        free(done);
        __atomic_add_fetch(&horizon_threads_reaped, 1, __ATOMIC_RELAXED);
        done = next;
    }
}

/* Plain C on a libnx thread, outside the records: it never runs Rust code. */
static void horizon_reaper_main(void *arg) {
    (void)arg;
    for (;;) {
        svcSleepThread(100000000ull); /* 100 ms */
        horizon_threads_reap();
    }
}

/* Starts the reaper on the first detach (the lock held). */
static void horizon_reaper_start(void) {
    if (horizon_reaper_started || horizon_reaper_off) return;
    if (R_SUCCEEDED(threadCreate(&horizon_reaper, horizon_reaper_main, NULL, NULL, 0x4000, 0x2C, -2)) &&
        R_SUCCEEDED(threadStart(&horizon_reaper))) {
        horizon_reaper_started = true;
    }
}

int __wrap_pthread_create(pthread_t *thread, const pthread_attr_t *attr, void *(*fn)(void *), void *arg) {
    struct horizon_thread *t = calloc(1, sizeof *t);
    if (!t) return EAGAIN;
    t->fn = fn;
    t->arg = arg;
    mutexLock(&horizon_threads_lock);
    int rc = __real_pthread_create(thread, attr, horizon_thread_entry, t);
    if (rc == 0) {
        t->id = *thread;
        t->has_id = true;
        t->next = horizon_threads;
        horizon_threads = t;
        __atomic_add_fetch(&horizon_threads_created, 1, __ATOMIC_RELAXED);
    }
    mutexUnlock(&horizon_threads_lock);
    if (rc != 0) free(t);
    return rc;
}

int __wrap_pthread_detach(pthread_t thread) {
    mutexLock(&horizon_threads_lock);
    struct horizon_thread *t = horizon_threads;
    while (t && !(t->has_id && pthread_equal(t->id, thread))) t = t->next;
    if (t) {
        t->detached = true;
        horizon_reaper_start();
    }
    mutexUnlock(&horizon_threads_lock);
    return t ? 0 : ESRCH;
}

/* The record leaves the list before the join: the join frees the thread's pthread_t (a heap
 * block), and a thread created on another core right then can get the same block, so the same
 * pthread_t. Taken afterwards, the lookup found that new thread's record and freed it while
 * the thread still had to write `finished` into it. */
int __wrap_pthread_join(pthread_t thread, void **retval) {
    mutexLock(&horizon_threads_lock);
    struct horizon_thread *t = horizon_thread_take(thread);
    mutexUnlock(&horizon_threads_lock);
    int rc = __real_pthread_join(thread, retval);
    if (rc == 0) {
        free(t);
        __atomic_add_fetch(&horizon_threads_joined, 1, __ATOMIC_RELAXED);
    } else if (t) {
        mutexLock(&horizon_threads_lock);
        t->next = horizon_threads;
        horizon_threads = t;
        mutexUnlock(&horizon_threads_lock);
    }
    return rc;
}

/* ---- heartbeat and stall dump ---- */

/* The game has twice ended without a crash report or a line in its log. A thread of the shim's
 * own, which takes no lock the game takes (no malloc, no newlib stdio, the SD card through fs
 * directly), writes every second how long the program has been up and how far its main loop has
 * got (horizon-heartbeat.txt: an ended program stops it, a stuck one doesn't). When the main
 * loop has not turned for 15 s it pauses each thread it knows in turn and appends where each
 * one stands (pc, lr and the frame-pointer chain, as offsets in the ELF for addr2line) to
 * horizon-stall.log. */
unsigned long horizon_switch_frames; /* main-loop turns, counted in input.c */

static Thread horizon_monitor;
static FsFileSystem *horizon_sd;
static char horizon_dump[64 << 10];
static size_t horizon_dump_len;

static void horizon_dump_str(const char *s) {
    while (*s && horizon_dump_len < sizeof horizon_dump - 1) horizon_dump[horizon_dump_len++] = *s++;
}

static void horizon_dump_hex(u64 v) {
    char b[20];
    int i = sizeof b - 1;
    b[i] = 0;
    do {
        b[--i] = "0123456789abcdef"[v & 15];
        v >>= 4;
    } while (v);
    b[--i] = 'x';
    b[--i] = '0';
    horizon_dump_str(b + i);
}

static void horizon_dump_dec(u64 v) {
    char b[24];
    int i = sizeof b - 1;
    b[i] = 0;
    do {
        b[--i] = (char)('0' + v % 10);
        v /= 10;
    } while (v);
    horizon_dump_str(b + i);
}

/* An address in the program as an ELF offset (+0x...), anything else as it is. */
static void horizon_dump_code(u64 addr) {
    u64 base = (u64)horizon_switch_code_base();
    if (addr >= base && addr - base < 0x10000000) {
        horizon_dump_str("+");
        horizon_dump_hex(addr - base);
    } else {
        horizon_dump_hex(addr);
    }
}

static void horizon_sd_write(const char *path, const char *data, size_t len, bool append) {
    static char fs_path[FS_MAX_PATH];
    if (!horizon_sd) return;
    strncpy(fs_path, path, sizeof fs_path - 1);
    fsFsCreateFile(horizon_sd, fs_path, 0, 0);
    FsFile f;
    if (R_FAILED(fsFsOpenFile(horizon_sd, fs_path, FsOpenMode_Write | FsOpenMode_Append, &f))) return;
    s64 at = 0;
    if (append) fsFileGetSize(&f, &at);
    fsFileWrite(&f, at, data, len, FsWriteOption_Flush);
    if (!append) fsFileSetSize(&f, (s64)len);
    fsFileClose(&f);
}

/* Pauses the thread, notes its registers and frame chain, lets it go on. */
static void horizon_dump_thread(const char *label, Handle h) {
    horizon_dump_str(label);
    Result rc = svcSetThreadActivity(h, ThreadActivity_Paused);
    if (R_FAILED(rc)) {
        horizon_dump_str(": could not pause it, ");
        horizon_dump_hex(rc);
        horizon_dump_str("\n");
        return;
    }
    ThreadContext ctx;
    for (int i = 0; i < 50; i++) {
        rc = svcGetThreadContext3(&ctx, h);
        if (R_SUCCEEDED(rc)) break;
        svcSleepThread(1000000ull);
    }
    if (R_FAILED(rc)) {
        horizon_dump_str(": no registers, ");
        horizon_dump_hex(rc);
        horizon_dump_str("\n");
    } else {
        horizon_dump_str(": pc ");
        horizon_dump_code(ctx.pc.x);
        horizon_dump_str(" lr ");
        horizon_dump_code(ctx.lr);
        horizon_dump_str(" sp ");
        horizon_dump_hex(ctx.sp);
        horizon_dump_str("\n");
        /* The frame chain, within the memory block that holds the stack. */
        MemoryInfo mi;
        u32 pi;
        u64 lo = 0, hi = 0;
        if (R_SUCCEEDED(svcQueryMemory(&mi, &pi, ctx.sp))) {
            lo = mi.addr;
            hi = mi.addr + mi.size;
        }
        u64 fp = ctx.fp;
        for (int n = 0; n < 48 && fp >= lo && fp + 16 <= hi && !(fp & 7); n++) {
            const u64 *frame = (const u64 *)fp;
            horizon_dump_str("    ");
            horizon_dump_code(frame[1]);
            horizon_dump_str("\n");
            if (frame[0] <= fp) break;
            fp = frame[0];
        }
    }
    svcSetThreadActivity(h, ThreadActivity_Runnable);
}

static void horizon_stall_dump(u64 up, u64 still) {
    horizon_dump_len = 0;
    horizon_dump_str("Horizon: ");
    horizon_dump_dec(up);
    horizon_dump_str(" s up, the main loop has not turned for ");
    horizon_dump_dec(still);
    horizon_dump_str(" s (+0x... are offsets in the ELF)\n");
    horizon_dump_thread("main thread", envGetMainThreadHandle());
    if (horizon_reaper_started) horizon_dump_thread("reaper", horizon_reaper.handle);
    bool locked = false;
    for (int i = 0; i < 100 && !(locked = mutexTryLock(&horizon_threads_lock)); i++) svcSleepThread(10000000ull);
    if (!locked) {
        horizon_dump_str("the thread records' lock stayed taken: the other threads are not listed\n");
    } else {
        for (struct horizon_thread *t = horizon_threads; t; t = t->next) {
            if (!t->has_id) continue;
            /* pthread_t is libnx's struct __pthread_t, which starts with its Thread, which
             * starts with the thread's handle. */
            horizon_dump_str(__atomic_load_n(&t->finished, __ATOMIC_ACQUIRE) ? "thread (finished)"
                             : t->detached                                    ? "thread (detached)"
                                                                              : "thread");
            horizon_dump_thread("", *(const Handle *)t->id);
        }
        mutexUnlock(&horizon_threads_lock);
    }
    horizon_dump_str("\n");
    horizon_sd_write("/switch/horizon/horizon-stall.log", horizon_dump, horizon_dump_len, true);
}

static void horizon_monitor_main(void *arg) {
    (void)arg;
    const u64 start = armGetSystemTick();
    unsigned long last_frames = 0;
    u64 last_turn = start, last_dump = 0;
    for (;;) {
        svcSleepThread(1000000000ull);
        const u64 now = armGetSystemTick();
        const u64 up = armTicksToNs(now - start) / 1000000000ull;
        const unsigned long frames = __atomic_load_n(&horizon_switch_frames, __ATOMIC_RELAXED);
        if (frames != last_frames) {
            last_frames = frames;
            last_turn = now;
        }
        const u64 still = armTicksToNs(now - last_turn) / 1000000000ull;
        horizon_dump_len = 0;
        horizon_dump_dec(up);
        horizon_dump_str(" s up; main loop ");
        horizon_dump_dec(frames);
        horizon_dump_str(" turns, last one ");
        horizon_dump_dec(still);
        horizon_dump_str(" s ago; threads: ");
        horizon_dump_dec(__atomic_load_n(&horizon_threads_created, __ATOMIC_RELAXED));
        horizon_dump_str(" created, ");
        horizon_dump_dec(__atomic_load_n(&horizon_threads_joined, __ATOMIC_RELAXED));
        horizon_dump_str(" joined, ");
        horizon_dump_dec(__atomic_load_n(&horizon_threads_reaped, __ATOMIC_RELAXED));
        horizon_dump_str(horizon_reaper_off ? " reaped (reaper off)\n" : " reaped\n");
        horizon_sd_write("/switch/horizon/horizon-heartbeat.txt", horizon_dump, horizon_dump_len, false);
        if (frames > 0 && still >= 15 && (last_dump == 0 || armTicksToNs(now - last_dump) >= 60000000000ull)) {
            horizon_stall_dump(up, still);
            last_dump = now;
        }
    }
}

static void horizon_monitor_start(void) {
    struct stat st;
    horizon_reaper_off = stat("sdmc:/switch/horizon/no-reaper", &st) == 0;
    horizon_sd = fsdevGetDeviceFileSystem("sdmc");
    if (horizon_sd) fsFsDeleteFile(horizon_sd, "/switch/horizon/horizon-stall.log");
    if (R_SUCCEEDED(threadCreate(&horizon_monitor, horizon_monitor_main, NULL, NULL, 0x4000, 0x2B, -2))) {
        threadStart(&horizon_monitor);
    }
}

/* rename over an existing file (POSIX replaces it): the Switch's file systems refuse with
 * EEXIST, which broke every save after a world's first (the game writes a temporary and
 * renames it over the file). Linked with --wrap=rename. */
int __real_rename(const char *from, const char *to);

int __wrap_rename(const char *from, const char *to) {
    int rc = __real_rename(from, to);
    if (rc != 0 && errno == EEXIST) {
        struct stat st;
        if (stat(to, &st) == 0 && !S_ISDIR(st.st_mode) && unlink(to) == 0) {
            rc = __real_rename(from, to);
        }
    }
    return rc;
}

/* For libnx threads created directly (threadCreate) that run Rust code. */
void horizon_switch_thread_enter(void) { horizon_sync_tp(); }

/* ---- POSIX gaps ---- */

ssize_t getrandom(void *buf, size_t len, unsigned int flags) {
    (void)flags;
    randomGet(buf, len);
    return (ssize_t)len;
}

long sysconf(int name) {
    switch (name) {
    case _SC_PAGESIZE:
        return 0x1000;
#ifdef _SC_NPROCESSORS_CONF
    case _SC_NPROCESSORS_CONF:
#endif
    case _SC_NPROCESSORS_ONLN:
        return 3; /* cores 0-2 belong to the application; core 3 is the system's */
    default:
        errno = EINVAL;
        return -1;
    }
}

int posix_memalign(void **out, size_t align, size_t size) {
    if (align < sizeof(void *)) align = sizeof(void *);
    void *p = memalign(align, size);
    if (!p) return ENOMEM;
    *out = p;
    return 0;
}

/* pread/pwrite (std's FileExt::read_at/write_at: region files). newlib has neither: seek,
 * transfer, seek back, under one lock so two threads reading one file at their own offsets
 * don't move each other's position. */
static pthread_mutex_t horizon_pio_lock = PTHREAD_MUTEX_INITIALIZER;

ssize_t pread(int fd, void *buf, size_t count, off_t offset) {
    pthread_mutex_lock(&horizon_pio_lock);
    off_t saved = lseek(fd, 0, SEEK_CUR);
    ssize_t n = -1;
    if (saved >= 0 && lseek(fd, offset, SEEK_SET) >= 0) {
        n = read(fd, buf, count);
        int err = errno;
        lseek(fd, saved, SEEK_SET);
        errno = err;
    }
    pthread_mutex_unlock(&horizon_pio_lock);
    return n;
}

ssize_t pwrite(int fd, const void *buf, size_t count, off_t offset) {
    pthread_mutex_lock(&horizon_pio_lock);
    off_t saved = lseek(fd, 0, SEEK_CUR);
    ssize_t n = -1;
    if (saved >= 0 && lseek(fd, offset, SEEK_SET) >= 0) {
        n = write(fd, buf, count);
        int err = errno;
        lseek(fd, saved, SEEK_SET);
        errno = err;
    }
    pthread_mutex_unlock(&horizon_pio_lock);
    return n;
}

/* No dynamic loading on the Switch: everything is linked statically. Crates that probe
 * for a library (wgpu-hal's libwayland-egl) see it missing. */
void *dlopen(const char *file, int mode) {
    (void)file;
    (void)mode;
    return NULL;
}
void *dlsym(void *handle, const char *name) {
    (void)handle;
    (void)name;
    return NULL;
}
int dlclose(void *handle) {
    (void)handle;
    return 0;
}
char *dlerror(void) { return (char *)"dynamic loading is not available on the Nintendo Switch"; }

int pthread_setname_np(pthread_t thread, const char *name) {
    (void)thread;
    (void)name;
    return 0;
}

/* ---- startup ---- */

static int horizon_log_fd = -1;
static bool horizon_sockets_up = false;
static bool horizon_nxlink = false;

/* Larger socket buffers than libnx's default: chunk data from a server arrives in bursts. */
static const SocketInitConfig horizon_socket_config = {
    .tcp_tx_buf_size = 0x8000,
    .tcp_rx_buf_size = 0x20000,
    .tcp_tx_buf_max_size = 0x40000,
    .tcp_rx_buf_max_size = 0x80000,
    .udp_tx_buf_size = 0x2400,
    .udp_rx_buf_size = 0xA500,
    .sb_efficiency = 8,
    .num_bsd_sessions = 3,
    .bsd_service_type = BsdServiceType_User,
};

void userAppInit(void) {
    horizon_sync_tp();
    if (R_SUCCEEDED(socketInitialize(&horizon_socket_config))) {
        horizon_sockets_up = true;
        horizon_nxlink = nxlinkStdio() >= 0;
    }
    romfsInit();
    horizon_monitor_start();
    /* The startup trace (spans.rs): which step each thread is in, for a start that stalls. */
    setenv("HORIZON_STARTUP_TRACE", "/switch/horizon/startup-trace.json", 0);
}

/* Sends stdout and stderr to `path` (unless nxlink already has them). Called from Rust
 * early in main; until then output goes to libnx's null device. */
void horizon_switch_log_to_file(const char *path) {
    if (horizon_nxlink || horizon_log_fd >= 0) return;
    mkdir("sdmc:/switch", 0777);
    mkdir("sdmc:/switch/horizon", 0777);
    horizon_log_fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0666);
    if (horizon_log_fd >= 0) {
        dup2(horizon_log_fd, STDOUT_FILENO);
        dup2(horizon_log_fd, STDERR_FILENO);
    }
}

void horizon_switch_log_flush(void) {
    if (horizon_log_fd >= 0) fsync(horizon_log_fd);
}

/* A line for the log that is on the card before the next instruction runs. */
void horizon_switch_note(const char *line) {
    write(STDERR_FILENO, line, strlen(line));
    if (horizon_log_fd >= 0) fsync(horizon_log_fd);
}

/* An address in the program as an offset in its ELF, for addr2line. */
void *horizon_switch_code_offset(void *addr) {
    return (void *)((char *)addr - (char *)horizon_switch_code_base());
}

static void horizon_note_end(const char *what, int code, void *caller) {
    char line[160];
    snprintf(line, sizeof line, "Horizon: %s(%d) called from %p (ELF offset %p)\n", what, code, caller,
             horizon_switch_code_offset(caller));
    horizon_switch_note(line);
}

/* How the program ends (linked with --wrap=exit and --wrap=abort). Both first leave a line with
 * their caller's offset in the ELF.
 *
 * newlib's exit returns, through libnx, to the Homebrew Menu's loader, which unmaps the program
 * while its other threads (dozens: workers, audio, the shim's own) still run; the next one
 * scheduled faults in the loader's process, an error screen on every quit. exit ends the
 * process instead, which stops them all; the console goes back to the HOME Menu. abort breaks
 * into the system's crash handler (Atmosphère writes a report with every thread's stack). */
void __wrap_exit(int code) {
    horizon_note_end("exit", code, __builtin_return_address(0));
    svcExitProcess();
}

void __wrap_abort(void) {
    horizon_note_end("abort", 0, __builtin_return_address(0));
    diagAbortWithResult(MAKERESULT(Module_Libnx, LibnxError_ShouldNotHappen));
}

void userAppExit(void) {
    romfsExit();
    if (horizon_log_fd >= 0) {
        fsync(horizon_log_fd);
        close(horizon_log_fd);
    }
    if (horizon_sockets_up) socketExit();
}

/* ---- queries for Rust ---- */

bool horizon_switch_nxlink(void) { return horizon_nxlink; }

/* True when started as an applet (from the album): a small memory pool. */
bool horizon_switch_is_applet(void) {
    AppletType t = appletGetAppletType();
    return t != AppletType_Application && t != AppletType_SystemApplication;
}

/* Memory the process may use and is using, in bytes. */
void horizon_switch_memory(uint64_t *total, uint64_t *used) {
    svcGetInfo(total, InfoType_TotalMemorySize, CUR_PROCESS_HANDLE, 0);
    svcGetInfo(used, InfoType_UsedMemorySize, CUR_PROCESS_HANDLE, 0);
}

/* Bytes newlib's malloc has handed out and not taken back (the heap itself is reserved
 * whole at start, so the process's used memory says nothing). */
uint64_t horizon_switch_heap_in_use(void) {
    struct mallinfo info = mallinfo();
    return (uint64_t)info.uordblks;
}

/* The calling thread's priority (0x2C is a normal thread's; lower numbers run first). */
bool horizon_switch_set_priority(int priority) {
    return R_SUCCEEDED(svcSetThreadPriority(CUR_THREAD_HANDLE, priority));
}

/* The SD card's free and total bytes. */
bool horizon_switch_sd_space(uint64_t *free_bytes, uint64_t *total_bytes) {
    FsFileSystem *fs = fsdevGetDeviceFileSystem("sdmc");
    s64 free_space = 0, total_space = 0;
    if (!fs) return false;
    if (R_FAILED(fsFsGetFreeSpace(fs, "/", &free_space))) return false;
    if (R_FAILED(fsFsGetTotalSpace(fs, "/", &total_space))) return false;
    *free_bytes = (uint64_t)free_space;
    *total_bytes = (uint64_t)total_space;
    return true;
}
