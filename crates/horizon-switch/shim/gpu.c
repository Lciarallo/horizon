/*
 * Guards around devkitPro's libdrm_nouveau 1.0.1, the layer under switch-mesa 20.1's nvc0
 * driver that builds the GPU's command submissions (linked with --wrap=nouveau_pushbuf_space,
 * --wrap=nouveau_pushbuf_data and --wrap=nvGpuChannelKickoff).
 *
 * A submission lists the pieces of command stream the GPU fetches ("pushes": the inline
 * commands, and buffers such as an indirect draw's arguments) in a fixed array of 512.
 * nouveau_pushbuf_data appends one without looking at the count: the caller is trusted to have
 * reserved it through nouveau_pushbuf_space, which submits first when the reservation would
 * not fit. One push past the array lands on the count of referenced buffers that follows it.
 * Set to 0 there, it makes nouveau_pushbuf_space wait for a submission it then never makes
 * (it submits only when buffers are referenced) and call itself through pushbuf_validate
 * until the stack runs out: the game twice crashed in exactly that loop. Every reservation
 * from Mesa is therefore asked for with a margin, so the submission comes before the array
 * is that full, and a push that still finds it full is reported with its caller.
 *
 * When the submission itself fails, libdrm's pushbuf_submit goes on to list the commands it
 * could not send, a loop whose printing (and so its progress) is compiled out: the thread
 * spins for ever. The failure is reported and the submission's entries are left queued in
 * the channel for the next one instead.
 */
#include <switch.h>
#include <stdint.h>
#include <stdio.h>
#include <nouveau.h>

void horizon_switch_note(const char *line);
void *horizon_switch_code_offset(void *addr);

/* libdrm_nouveau's private layout, read from its code in this build: the current submission
 * record sits at +72 in the push buffer, and holds 1024 buffers then 512 pushes of 24 bytes,
 * then the two counts. */
#define HORIZON_PUSHBUF_KREC 72
#define HORIZON_KREC_NR_BUFFER 0x9008
#define HORIZON_KREC_NR_PUSH 0x900c
#define HORIZON_MAX_PUSH 512
/* Pushes asked for on top of each reservation: a submission comes at most this much earlier. */
#define HORIZON_PUSH_MARGIN 16

static const int *horizon_krec_counts(struct nouveau_pushbuf *push) {
    const char *krec = *(char *const *)((const char *)push + HORIZON_PUSHBUF_KREC);
    return (const int *)(krec + HORIZON_KREC_NR_BUFFER);
}

/* At most a few lines per kind of event: the log is for finding the caller, not counting. */
static int horizon_gpu_notes;

static void horizon_gpu_note(const char *what, struct nouveau_pushbuf *push, void *caller) {
    if (horizon_gpu_notes >= 20) return;
    horizon_gpu_notes++;
    const int *n = horizon_krec_counts(push);
    char line[200];
    snprintf(line, sizeof line, "Horizon GPU: %s (%d pushes, %d buffers), called from ELF offset %p\n", what, n[1],
             n[0], horizon_switch_code_offset(caller));
    horizon_switch_note(line);
}

int __real_nouveau_pushbuf_space(struct nouveau_pushbuf *push, uint32_t dwords, uint32_t relocs, uint32_t pushes);
void __real_nouveau_pushbuf_data(struct nouveau_pushbuf *push, struct nouveau_bo *bo, uint64_t offset,
                                 uint64_t length);
Result __real_nvGpuChannelKickoff(void *channel);

int __wrap_nouveau_pushbuf_space(struct nouveau_pushbuf *push, uint32_t dwords, uint32_t relocs, uint32_t pushes) {
    if (horizon_krec_counts(push)[1] > HORIZON_MAX_PUSH) {
        horizon_gpu_note("the push list has overflowed", push, __builtin_return_address(0));
    }
    return __real_nouveau_pushbuf_space(push, dwords, relocs, pushes + HORIZON_PUSH_MARGIN);
}

void __wrap_nouveau_pushbuf_data(struct nouveau_pushbuf *push, struct nouveau_bo *bo, uint64_t offset,
                                 uint64_t length) {
    /* This call can add two pushes (the inline commands written since the last one, then
     * `bo`'s), and the submission one more for the commands after it. */
    if (bo && horizon_krec_counts(push)[1] + 3 > HORIZON_MAX_PUSH) {
        horizon_gpu_note("a push without room reserved for it", push, __builtin_return_address(0));
    }
    __real_nouveau_pushbuf_data(push, bo, offset, length);
}

Result __wrap_nvGpuChannelKickoff(void *channel) {
    static Result last;
    static unsigned in_row;
    Result rc = __real_nvGpuChannelKickoff(channel);
    in_row = rc == last ? in_row + 1 : 1;
    last = rc;
    if (R_SUCCEEDED(rc)) return rc;
    if (in_row == 1 || in_row % 100 == 0) {
        char line[120];
        snprintf(line, sizeof line, "Horizon GPU: a submission failed with %#x (2%03u-%04u), %u times in a row\n", rc,
                 R_MODULE(rc), R_DESCRIPTION(rc), in_row);
        horizon_switch_note(line);
    }
    return 0;
}
