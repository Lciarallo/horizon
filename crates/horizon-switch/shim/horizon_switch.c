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

/* ---- threads ---- */

/* libnx has no pthread_detach (newlib's answers ENOSYS), so a detached thread was never
 * closed: its stack and kernel thread stayed until the process ran out of threads (ENOMEM
 * from every spawn after a few minutes, std dropping a JoinHandle being a detach). Every
 * thread therefore keeps a record until it is joined; a detached one is joined by the next
 * pthread_create or pthread_detach once it has finished. Linked with --wrap=pthread_detach
 * and --wrap=pthread_join. */
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

/* Joins the detached threads that have finished. */
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
        done = next;
    }
}

int __wrap_pthread_create(pthread_t *thread, const pthread_attr_t *attr, void *(*fn)(void *), void *arg) {
    horizon_threads_reap();
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
    }
    mutexUnlock(&horizon_threads_lock);
    if (rc != 0) free(t);
    return rc;
}

int __wrap_pthread_detach(pthread_t thread) {
    mutexLock(&horizon_threads_lock);
    struct horizon_thread *t = horizon_threads;
    while (t && !(t->has_id && pthread_equal(t->id, thread))) t = t->next;
    if (t) t->detached = true;
    mutexUnlock(&horizon_threads_lock);
    horizon_threads_reap();
    return t ? 0 : ESRCH;
}

int __wrap_pthread_join(pthread_t thread, void **retval) {
    int rc = __real_pthread_join(thread, retval);
    if (rc == 0) {
        mutexLock(&horizon_threads_lock);
        struct horizon_thread *t = horizon_thread_take(thread);
        mutexUnlock(&horizon_threads_lock);
        free(t);
    }
    return rc;
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
