// ============================================================================
// doomgeneric_tg.c - rCore-Tutorial 平台适配层
//
// 实现 doomgeneric 要求的 5 个接口函数：
//   DG_Init, DG_DrawFrame, DG_SleepMs, DG_GetTicksMs, DG_GetKey
//
// 键盘映射：
//   WASD: 移动/转向
//   J: 射击
//   Q: 退出/菜单
//   Space/Enter: 确认
// ============================================================================

#include "doomgeneric.h"
#include "doomkeys.h"
#include "m_controls.h"
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>

// 来自 Rust lib.rs 的 FFI 函数
extern int32_t tg_set_input_mode_polling(void);
extern int32_t tg_framebuffer_info(void *out);
extern int32_t tg_framebuffer_flush(void);
extern uint32_t tg_get_ticks_ms(void);
extern void tg_sleep_ms(uint32_t ms);
extern int32_t tg_getchar_poll(void);

// ============================================================================
// DOOM 调色板（RGB888，从 WAD 的 PLAYPAL lump 加载）
// 默认调色板（fallback，在 I_SetPalette 被调用前使用）
// 这是标准 DOOM 调色板的近似值
// ============================================================================
unsigned char DG_Palette[256 * 3] = {
    // 标准 DOOM 调色板索引 0-31（主要颜色）
    0,0,0,    31,0,0,    0,31,0,    0,0,31,    // 0-3
    31,31,0,  0,31,31,   31,0,31,   16,16,16,  // 4-7
    63,63,63, 127,63,63,  63,127,63,  63,63,127, // 8-11
    127,127,63, 63,127,127,127,63,127, 95,95,95, // 12-15
    47,47,47,  31,31,31,  0,0,0,     0,0,0,     // 16-19
    0,0,0,    0,0,0,     0,0,0,     0,0,0,     // 20-23
    0,0,0,    0,0,0,     0,0,0,     0,0,0,     // 24-27
    0,0,0,    0,0,0,     0,0,0,     0,0,0,     // 28-31
};

// ============================================================================
// Framebuffer 状态
// ============================================================================

typedef struct {
    uint8_t *ptr;
    uintptr_t len;
    uintptr_t width;
    uintptr_t height;
    uintptr_t pitch;
} TgFbInfo;

static uint8_t *g_fb = NULL;       // 目标 framebuffer 指针
static size_t  g_fb_len = 0;       // framebuffer 可用长度
static size_t  g_fb_w = 0;         // framebuffer 宽度
static size_t  g_fb_h = 0;         // framebuffer 高度
static size_t  g_fb_pitch = 0;     // 每行字节数（含 stride 填充）

// ============================================================================
// 按键队列（轮询模式）
// ============================================================================

static unsigned short g_key_queue[64];
static unsigned int   g_key_write = 0;
static unsigned int   g_key_read  = 0;
// 当前认为正在按住的 Doom key（-1 表示无按住状态）
static int           g_pending_key = -1;
// 按住时，延迟这么久才触发 release（避免快速松开）
static uint32_t      g_release_deadline = 0;

// ASCII -> Doom key 映射
static uint8_t map_ascii_to_doom(uint8_t ch) {
    switch (ch) {
        case 'q': case 'Q': return KEY_ESCAPE;
        case '\n': case '\r': return KEY_ENTER;
        case ' ': return KEY_USE;  // 空格映射为使用键
        case 'j': case 'J': return KEY_FIRE;
        // WASD 方向键映射（用于菜单和移动）
        case 'w': case 'W': return KEY_UPARROW;    // 0xad - 前进/上
        case 's': case 'S': return KEY_DOWNARROW;  // 0xaf - 后退/下
        case 'a': case 'A': return KEY_LEFTARROW;  // 0xac - 左转/左
        case 'd': case 'D': return KEY_RIGHTARROW; // 0xae - 右转/右
        default:  return ch;
    }
}

static void push_key(int pressed, uint8_t doom_key) {
    unsigned short data = (unsigned short)((pressed << 8) | doom_key);
    g_key_queue[g_key_write] = data;
    g_key_write = (g_key_write + 1u) % 64u;
}

// 轮询一次键盘：将 UART 输入转换为 Doom key 事件
static void poll_keys_once(void) {
    uint32_t now = tg_get_ticks_ms();
    int32_t raw = tg_getchar_poll();

    if (raw >= 0 && raw <= 255) {
        uint8_t key = map_ascii_to_doom((uint8_t)raw);

        // 如果是同一按键：延长按住时间，不触发 release
        if (g_pending_key == (int)key) {
            g_release_deadline = now + 100;
            return;
        }

        // 新按键：先释放旧按键
        if (g_pending_key >= 0) {
            push_key(0, (uint8_t)g_pending_key);
        }

        // 报 Press 事件并进入按住态
        push_key(1, key);
        g_pending_key = (int)key;
        g_release_deadline = now + 100;
        return;
    }

    // 无输入时，到达 deadline 才释放
    if (g_pending_key >= 0 && (int32_t)(now - g_release_deadline) >= 0) {
        push_key(0, (uint8_t)g_pending_key);
        g_pending_key = -1;
    }
}

// ============================================================================
// 像素操作（BGRA 格式）
// ============================================================================

static void put_px(size_t x, size_t y, uint32_t color) {
    if (g_fb == NULL || x >= g_fb_w || y >= g_fb_h || g_fb_pitch == 0) return;
    size_t idx = y * g_fb_pitch + x * 4;
    if (idx + 4 > g_fb_len) return;
    g_fb[idx + 0] = (uint8_t)(color & 0xffu);        // B
    g_fb[idx + 1] = (uint8_t)((color >> 8) & 0xffu);  // G
    g_fb[idx + 2] = (uint8_t)((color >> 16) & 0xffu); // R
    g_fb[idx + 3] = 0xffu;                             // A
}

// 使用调色板索引设置像素
static void put_px_palette(size_t x, size_t y, uint8_t color_idx) {
    if (x >= DOOMGENERIC_RESX || y >= DOOMGENERIC_RESY) return;
    unsigned char r = DG_Palette[color_idx * 3 + 0];
    unsigned char g2 = DG_Palette[color_idx * 3 + 1];
    unsigned char b2 = DG_Palette[color_idx * 3 + 2];
    uint32_t color = (0xFFu << 24) | (r << 16) | (g2 << 8) | b2;
    put_px(x, y, color);
}

static void clear_fb(uint32_t color) {
    if (g_fb == NULL) return;
    for (size_t y = 0; y < g_fb_h; y++) {
        for (size_t x = 0; x < g_fb_w; x++) {
            put_px(x, y, color);
        }
    }
}

// ============================================================================
// DOOMgeneric 五个接口实现
// ============================================================================

void DG_Init(void) {
    // 设置轮询输入模式
    tg_set_input_mode_polling();

    // 获取 framebuffer 信息
    TgFbInfo info;
    info.ptr = NULL;
    info.len = 0;
    info.width = 0;
    info.height = 0;

    int32_t fb_ret = tg_framebuffer_info(&info);
    printf("[DEBUG] DG_Init: fb ret=%d ptr=%p %zux%zu pitch=%zu len=%zu\n",
           fb_ret, (void*)info.ptr, info.width, info.height, info.pitch, info.len);

    if (fb_ret == 0 && info.ptr != NULL && info.len > 0 && info.width > 0) {
        g_fb = info.ptr;
        g_fb_len = info.len;
        g_fb_w = info.width;
        g_fb_h = info.height;
        g_fb_pitch = info.pitch != 0 ? info.pitch : (info.width * 4);
    }
}

void DG_DrawFrame(void) {
    if (g_fb == NULL || DG_ScreenBuffer == NULL || g_fb_pitch == 0) return;

    /*
     * I_FinishUpdate() 已通过 cmap_to_fb() 把 I_VideoBuffer（8 位索引）写成
     * 32 位像素写入 DG_ScreenBuffer（与 I_InitGraphics 里 s_Fb 的 RGBA 位域一致，
     * 小端内存布局为 [B,G,R,X]）。
     * 此处必须按 uint32_t 读取，不能再当作调色板索引，否则会出现竖条花屏。
     */
    uint32_t *src_px = (uint32_t *)DG_ScreenBuffer;
    const size_t src_w = DOOMGENERIC_RESX;
    const size_t src_h = DOOMGENERIC_RESY;

    size_t copy_w = src_w;
    size_t copy_h = src_h;
    if (copy_w > g_fb_w) copy_w = g_fb_w;
    if (copy_h > g_fb_h) copy_h = g_fb_h;

    size_t dst_pitch = g_fb_pitch;

    size_t scale_x = g_fb_w / src_w;
    size_t scale_y = g_fb_h / src_h;
    if (scale_x == 0) scale_x = 1;
    if (scale_y == 0) scale_y = 1;

    for (size_t y = 0; y < g_fb_h; y++) {
        size_t row = y * dst_pitch;
        for (size_t x = 0; x < g_fb_w; x++) {
            size_t idx = row + x * 4;
            if (idx + 4 <= g_fb_len) {
                g_fb[idx + 0] = 0;
                g_fb[idx + 1] = 0;
                g_fb[idx + 2] = 0;
                g_fb[idx + 3] = 0xFF;
            }
        }
    }

    for (size_t sy = 0; sy < copy_h; sy++) {
        for (size_t sx = 0; sx < copy_w; sx++) {
            uint32_t s = src_px[sy * src_w + sx];
            for (size_t dy = 0; dy < scale_y; dy++) {
                for (size_t dx = 0; dx < scale_x; dx++) {
                    size_t dst_x = sx * scale_x + dx;
                    size_t dst_y = sy * scale_y + dy;
                    if (dst_x < g_fb_w && dst_y < g_fb_h) {
                        size_t idx = dst_y * dst_pitch + dst_x * 4;
                        if (idx + 4 <= g_fb_len) {
                            g_fb[idx + 0] = (uint8_t)(s & 0xFFu);
                            g_fb[idx + 1] = (uint8_t)((s >> 8) & 0xFFu);
                            g_fb[idx + 2] = (uint8_t)((s >> 16) & 0xFFu);
                            g_fb[idx + 3] = 0xFFu;
                        }
                    }
                }
            }
        }
    }

    tg_framebuffer_flush();
}

void DG_SleepMs(uint32_t ms) {
    tg_sleep_ms(ms);
}

uint32_t DG_GetTicksMs(void) {
    return tg_get_ticks_ms();
}

// 按键获取：从轮询队列中取出一个事件
int DG_GetKey(int *pressed, unsigned char *key) {
    poll_keys_once();
    if (g_key_read == g_key_write) return 0;
    unsigned short data = g_key_queue[g_key_read];
    g_key_read = (g_key_read + 1u) % 64u;
    *pressed = (int)(data >> 8);
    *key = (unsigned char)(data & 0xffu);
    return 1;
}

void DG_SetWindowTitle(const char *title) {
    (void)title;
    // 不需要实现
}
