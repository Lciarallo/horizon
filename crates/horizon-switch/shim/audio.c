/* Sound output for cpal's Nintendo Switch host (third_party/cpal: src/host/horizon), through
 * libnx's audout service: 16-bit stereo at the service's rate (48 kHz), a ring of buffers
 * the service plays in turn. One writer at a time (a lock), as cpal opens one stream. */
#include <switch.h>
#include <malloc.h>
#include <pthread.h>
#include <string.h>

#define HORIZON_AUDIO_BUFFERS 4

static pthread_mutex_t horizon_audio_lock = PTHREAD_MUTEX_INITIALIZER;
static bool horizon_audio_ready = false;
static AudioOutBuffer horizon_audio_bufs[HORIZON_AUDIO_BUFFERS];
static void *horizon_audio_mem[HORIZON_AUDIO_BUFFERS];
static bool horizon_audio_in_flight[HORIZON_AUDIO_BUFFERS];
static size_t horizon_audio_frames = 0;

/* Opens the output once (later calls only report it): frames per buffer, rate, channels. */
bool horizon_switch_audio_open(uint32_t frames, uint32_t *rate, uint32_t *channels) {
    pthread_mutex_lock(&horizon_audio_lock);
    if (!horizon_audio_ready) {
        if (R_FAILED(audoutInitialize())) {
            pthread_mutex_unlock(&horizon_audio_lock);
            return false;
        }
        size_t bytes = ((size_t)frames * 2 * sizeof(int16_t) + 0xFFF) & ~(size_t)0xFFF;
        for (int i = 0; i < HORIZON_AUDIO_BUFFERS; i++) {
            horizon_audio_mem[i] = memalign(0x1000, bytes);
            if (!horizon_audio_mem[i]) {
                pthread_mutex_unlock(&horizon_audio_lock);
                return false;
            }
            memset(horizon_audio_mem[i], 0, bytes);
            horizon_audio_bufs[i].next = NULL;
            horizon_audio_bufs[i].buffer = horizon_audio_mem[i];
            horizon_audio_bufs[i].buffer_size = bytes;
            horizon_audio_bufs[i].data_size = (u64)frames * 2 * sizeof(int16_t);
            horizon_audio_bufs[i].data_offset = 0;
            horizon_audio_in_flight[i] = false;
        }
        if (R_FAILED(audoutStartAudioOut())) {
            audoutExit();
            pthread_mutex_unlock(&horizon_audio_lock);
            return false;
        }
        horizon_audio_frames = frames;
        horizon_audio_ready = true;
    }
    *rate = audoutGetSampleRate();
    *channels = 2;
    pthread_mutex_unlock(&horizon_audio_lock);
    return true;
}

/* Frames each buffer holds (what `horizon_switch_audio_write` takes). */
uint32_t horizon_switch_audio_frames(void) { return (uint32_t)horizon_audio_frames; }

/* Plays one buffer of interleaved stereo samples (`horizon_switch_audio_frames` frames),
 * waiting for the service to give one back when all are queued. */
bool horizon_switch_audio_write(const int16_t *samples) {
    pthread_mutex_lock(&horizon_audio_lock);
    if (!horizon_audio_ready) {
        pthread_mutex_unlock(&horizon_audio_lock);
        return false;
    }
    int slot = -1;
    for (int i = 0; i < HORIZON_AUDIO_BUFFERS; i++) {
        if (!horizon_audio_in_flight[i]) { slot = i; break; }
    }
    while (slot < 0) {
        AudioOutBuffer *released = NULL;
        u32 count = 0;
        Result rc = audoutWaitPlayFinish(&released, &count, 100000000ULL /* 100 ms */);
        if (R_SUCCEEDED(rc) && released && count > 0) {
            for (int i = 0; i < HORIZON_AUDIO_BUFFERS; i++) {
                if (&horizon_audio_bufs[i] == released) {
                    horizon_audio_in_flight[i] = false;
                    slot = i;
                }
            }
        }
        /* More may have finished at once. */
        while (R_SUCCEEDED(audoutGetReleasedAudioOutBuffer(&released, &count)) && released && count > 0) {
            for (int i = 0; i < HORIZON_AUDIO_BUFFERS; i++) {
                if (&horizon_audio_bufs[i] == released) {
                    horizon_audio_in_flight[i] = false;
                    if (slot < 0) slot = i;
                }
            }
        }
    }
    size_t bytes = horizon_audio_frames * 2 * sizeof(int16_t);
    memcpy(horizon_audio_mem[slot], samples, bytes);
    armDCacheFlush(horizon_audio_mem[slot], bytes);
    horizon_audio_in_flight[slot] = true;
    bool ok = R_SUCCEEDED(audoutAppendAudioOutBuffer(&horizon_audio_bufs[slot]));
    if (!ok) horizon_audio_in_flight[slot] = false;
    pthread_mutex_unlock(&horizon_audio_lock);
    return ok;
}
