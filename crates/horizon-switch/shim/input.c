/* Controller, touch screen and applet state for Rust: libnx's pad API is inline functions. */
#include <switch.h>
#include <string.h>
#include <sys/iosupport.h>

typedef struct {
    uint64_t held;
    uint64_t down;
    uint64_t up;
    int32_t lx, ly, rx, ry; /* -32767..32767, y up */
    uint32_t style_set;     /* HidNpadStyleTag bits of the controller in use */
    uint32_t attributes;    /* HidNpadAttribute bits */
    uint32_t connected;
    uint32_t touch_count;
    int32_t touch_x[4];
    int32_t touch_y[4];
    uint32_t touch_id[4];
} HorizonInput;

static PadState horizon_pad;
static bool horizon_input_ready = false;

void horizon_switch_input_init(void) {
    if (horizon_input_ready) return;
    /* One player: handheld, both Joy-Cons, a Pro Controller, or a single Joy-Con. */
    padConfigureInput(1, HidNpadStyleSet_NpadStandard);
    padInitializeDefault(&horizon_pad);
    hidInitializeTouchScreen();
    horizon_input_ready = true;
}

void horizon_switch_input_poll(HorizonInput *out) {
    memset(out, 0, sizeof *out);
    if (!horizon_input_ready) horizon_switch_input_init();
    padUpdate(&horizon_pad);
    out->held = padGetButtons(&horizon_pad);
    out->down = padGetButtonsDown(&horizon_pad);
    out->up = padGetButtonsUp(&horizon_pad);
    HidAnalogStickState l = padGetStickPos(&horizon_pad, 0);
    HidAnalogStickState r = padGetStickPos(&horizon_pad, 1);
    out->lx = l.x;
    out->ly = l.y;
    out->rx = r.x;
    out->ry = r.y;
    out->style_set = padGetStyleSet(&horizon_pad);
    out->attributes = padGetAttributes(&horizon_pad);
    out->connected = padIsConnected(&horizon_pad);

    HidTouchScreenState touch = {0};
    if (hidGetTouchScreenStates(&touch, 1)) {
        uint32_t n = touch.count > 4 ? 4 : touch.count;
        out->touch_count = n;
        for (uint32_t i = 0; i < n; i++) {
            out->touch_x[i] = touch.touches[i].x;
            out->touch_y[i] = touch.touches[i].y;
            out->touch_id[i] = touch.touches[i].finger_id;
        }
    }
}

/* The touch screen alone (winit's backend): no padUpdate, which would take the
 * controller's button edges from the game. */
void horizon_switch_touch_poll(HorizonInput *out) {
    memset(out, 0, sizeof *out);
    if (!horizon_input_ready) horizon_switch_input_init();
    HidTouchScreenState touch = {0};
    if (hidGetTouchScreenStates(&touch, 1)) {
        uint32_t n = touch.count > 4 ? 4 : touch.count;
        out->touch_count = n;
        for (uint32_t i = 0; i < n; i++) {
            out->touch_x[i] = touch.touches[i].x;
            out->touch_y[i] = touch.touches[i].y;
            out->touch_id[i] = touch.touches[i].finger_id;
        }
    }
}

/* 1280x720 handheld, 1920x1080 docked. */
void horizon_switch_display_size(uint32_t *w, uint32_t *h) {
    if (appletGetOperationMode() == AppletOperationMode_Console) {
        *w = 1920;
        *h = 1080;
    } else {
        *w = 1280;
        *h = 720;
    }
}

bool horizon_switch_main_loop(void) { return appletMainLoop(); }

/* The default window, for EGL (eglCreateWindowSurface takes the NWindow*). */
void *horizon_switch_default_window(void) { return nwindowGetDefault(); }

bool horizon_switch_focused(void) { return appletGetFocusState() == AppletFocusState_InFocus; }

/* consoleInit takes over the stdout device; it is put back on exit, so output that is a file or
 * nxlink (dup2 onto fd 1) goes on there afterwards. */
static const devoptab_t *horizon_saved_stdout = NULL;
static bool horizon_console_on = false;
void horizon_switch_console_init(void) {
    if (horizon_console_on) return;
    horizon_saved_stdout = devoptab_list[STD_OUT];
    consoleInit(NULL);
    horizon_console_on = true;
}
void horizon_switch_console_update(void) {
    if (horizon_console_on) consoleUpdate(NULL);
}
void horizon_switch_console_exit(void) {
    if (!horizon_console_on) return;
    consoleExit(NULL);
    devoptab_list[STD_OUT] = horizon_saved_stdout;
    horizon_console_on = false;
}
/* CPU boost while loading: core clock to 1785 MHz (ApmCpuBoostMode_FastLoad). */
void horizon_switch_boost(bool on) {
    appletSetCpuBoostMode(on ? ApmCpuBoostMode_FastLoad : ApmCpuBoostMode_Normal);
}

/* Text from the system keyboard (swkbd). Returns false when cancelled. */
bool horizon_switch_keyboard(const char *guide, const char *initial, char *out, size_t out_len) {
    SwkbdConfig kbd;
    if (R_FAILED(swkbdCreate(&kbd, 0))) return false;
    swkbdConfigMakePresetDefault(&kbd);
    if (guide) swkbdConfigSetGuideText(&kbd, guide);
    if (initial) swkbdConfigSetInitialText(&kbd, initial);
    Result rc = swkbdShow(&kbd, out, out_len);
    swkbdClose(&kbd);
    return R_SUCCEEDED(rc);
}
