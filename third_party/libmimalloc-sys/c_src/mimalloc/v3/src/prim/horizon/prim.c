/* ----------------------------------------------------------------------------
Nintendo Switch homebrew (libnx, newlib) primitives for mimalloc, added by Horizon's
Switch port (docs/switch.md). Based on the wasi and unix primitives.

libnx reserves the whole heap at start and has no mmap: mimalloc takes large blocks
from newlib's heap with memalign and gives them back with free. The blocks are few and
big (arenas, `MI_DEFAULT_ARENA_RESERVE`), so newlib's single lock is taken rarely; the
game's millions of small allocations stay in mimalloc's per-thread pages.
-----------------------------------------------------------------------------*/

// This file is included in `src/prim/prim.c`

#include "mimalloc.h"
#include "mimalloc/internal.h"
#include "mimalloc/prim.h"

#include <malloc.h>   // memalign
#include <pthread.h>  // thread-done keys
#include <sched.h>    // sched_yield
#include <stdio.h>    // fputs
#include <stdlib.h>   // getenv, free
#include <string.h>   // memset
#include <time.h>     // clock_gettime
#include <sys/types.h>

// The startup shim's (crates/horizon-switch/shim): libnx's csrng.
extern ssize_t getrandom(void* buf, size_t len, unsigned int flags);

void _mi_prim_mem_init( mi_os_mem_config_t* config ) {
  config->page_size = 4*MI_KiB;
  config->large_page_size = 0;
  config->alloc_granularity = 4*MI_KiB;
  config->has_overcommit = false;
  config->has_partial_free = false;    // free only what memalign gave, whole
  config->has_virtual_reserve = false; // no address space without memory
}

int _mi_prim_free(void* addr, size_t size ) {
  MI_UNUSED(size);
  free(addr);
  return 0;
}

int _mi_prim_alloc(void* hint_addr, size_t size, size_t try_alignment, bool commit, bool allow_large, bool* is_large, bool* is_zero, void** addr) {
  MI_UNUSED(hint_addr); MI_UNUSED(commit); MI_UNUSED(allow_large);
  *is_large = false;
  *is_zero = false;
  size_t align = try_alignment;
  if (align < 4*MI_KiB) align = 4*MI_KiB;
  *addr = memalign(align, size);
  return (*addr != NULL ? 0 : ENOMEM);
}

int _mi_prim_commit(void* addr, size_t size, bool* is_zero) {
  MI_UNUSED(addr); MI_UNUSED(size);
  *is_zero = false;
  return 0;
}

int _mi_prim_decommit(void* addr, size_t size, bool* needs_recommit) {
  MI_UNUSED(addr); MI_UNUSED(size);
  *needs_recommit = false;
  return 0;
}

int _mi_prim_reset(void* addr, size_t size) {
  MI_UNUSED(addr); MI_UNUSED(size);
  return 0;
}

int _mi_prim_reuse(void* addr, size_t size) {
  MI_UNUSED(addr); MI_UNUSED(size);
  return 0;
}

int _mi_prim_protect(void* addr, size_t size, bool protect) {
  MI_UNUSED(addr); MI_UNUSED(size); MI_UNUSED(protect);
  return 0;
}

int _mi_prim_alloc_huge_os_pages(void* hint_addr, size_t size, int numa_node, bool* is_zero, void** addr) {
  MI_UNUSED(hint_addr); MI_UNUSED(size); MI_UNUSED(numa_node);
  *is_zero = true;
  *addr = NULL;
  return ENOSYS;
}

size_t _mi_prim_numa_node(void) {
  return 0;
}

size_t _mi_prim_numa_node_count(void) {
  return 1;
}

mi_msecs_t _mi_prim_clock_now(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return ((mi_msecs_t)t.tv_sec * 1000) + ((mi_msecs_t)t.tv_nsec / 1000000);
}

void _mi_prim_process_info(mi_process_info_t* pinfo) {
  MI_UNUSED(pinfo); // defaults
}

void _mi_prim_out_stderr( const char* msg ) {
  fputs(msg,stderr);
}

bool _mi_prim_getenv(const char* name, char* result, size_t result_size) {
  if (_mi_preloading()) return false;
  const char* s = getenv(name);
  if (s == NULL) {
    char buf[64+1];
    size_t len = _mi_strnlen(name,sizeof(buf)-1);
    for (size_t i = 0; i < len; i++) {
      buf[i] = _mi_toupper(name[i]);
    }
    buf[len] = 0;
    s = getenv(buf);
  }
  if (s == NULL || _mi_strnlen(s,result_size) >= result_size)  return false;
  _mi_strlcpy(result, s, result_size);
  return true;
}

bool _mi_prim_random_buf(void* buf, size_t buf_len) {
  return getrandom(buf, buf_len, 0) == (ssize_t)buf_len;
}

// Thread ends, through a pthread key's destructor (as unix/prim.c with MI_USE_PTHREADS).
static pthread_key_t mi_horizon_done_key;
static bool mi_horizon_done_key_made = false;

static void mi_horizon_thread_done(void* value) {
  if (value != NULL) {
    _mi_thread_done((mi_theap_t*)value);
  }
}

void _mi_prim_thread_init_auto_done(void) {
  if (pthread_key_create(&mi_horizon_done_key, &mi_horizon_thread_done) == 0) {
    mi_horizon_done_key_made = true;
  }
}

void _mi_prim_thread_done_auto_done(void) {
  if (mi_horizon_done_key_made) {
    pthread_key_delete(mi_horizon_done_key);
    mi_horizon_done_key_made = false;
  }
}

void _mi_prim_thread_associate_default_theap(mi_theap_t* theap) {
  if (mi_horizon_done_key_made) {
    pthread_setspecific(mi_horizon_done_key, theap);
  }
}

bool _mi_prim_thread_is_in_threadpool(void) {
  return false;
}

void _mi_prim_thread_yield(void) {
  sched_yield();
}
