#include <stdint.h>
#include <stdarg.h>
#include <stddef.h>

typedef struct _IO_FILE FILE;

extern int32_t tg_sys_open(const char *path, int32_t flags);
extern int32_t tg_sys_close(int32_t fd);
extern int32_t tg_sys_read(int32_t fd, void *buf, uintptr_t len);
extern int32_t tg_sys_write(int32_t fd, const void *buf, uintptr_t len);
extern int32_t tg_sys_unlink(const char *path);
extern uint32_t tg_get_ticks_ms(void);
extern void *tg_alloc(uintptr_t size);

#define TG_O_RDONLY 0
#define TG_O_WRONLY 1
#define TG_O_RDWR 2
#define TG_O_CREATE (1 << 9)
#define TG_O_TRUNC (1 << 10)

#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

#define TG_MAX_FILES 16
#define TG_MAX_FILE_SIZE (32 * 1024 * 1024)
#define TG_PATH_MAX 256

typedef struct {
 int used;
 int mode_write;
 int fd;
 int err;
 int eof;
 unsigned char *data;
 size_t size;
 size_t pos;
 char path[TG_PATH_MAX];
 unsigned char write_buf[4096];
 size_t write_buf_len;
} TgFile;

static TgFile g_files[TG_MAX_FILES];
static struct {
 int valid;
 char path[TG_PATH_MAX];
 unsigned char *data;
 size_t size;
} g_read_cache;
static FILE *g_stdin = (FILE *)0;
static FILE *g_stdout = (FILE *)1;
static FILE *g_stderr = (FILE *)2;
FILE *stdin = (FILE *)0;
FILE *stdout = (FILE *)1;
FILE *stderr = (FILE *)2;

static size_t tg_dbg_strlen_local(const char *s) {
 size_t n = 0;
 if (!s) {
 return 0;
 }
 while (s[n]) {
 n++;
 }
 return n;
}

static int tg_dbg_contains_local(const char *haystack, const char *needle) {
 if (!haystack || !needle || !needle[0]) {
 return 0;
 }
 size_t nlen = tg_dbg_strlen_local(needle);
 size_t hlen = tg_dbg_strlen_local(haystack);
 if (nlen > hlen) {
 return 0;
 }
 for (size_t i = 0; i + nlen <= hlen; i++) {
 size_t j = 0;
 while (j < nlen && haystack[i + j] == needle[j]) {
 j++;
 }
 if (j == nlen) {
 return 1;
 }
 }
 return 0;
}

static void tg_dbg_raw(const char *s) {
 if (!s) {
 return;
 }
 tg_sys_write(1, s, (uintptr_t)tg_dbg_strlen_local(s));
}

static void tg_dbg_i32(int32_t v) {
 char buf[16];
 int i = 0;
 unsigned int x;
 if (v == 0) {
 tg_dbg_raw("0");
 return;
 }
 if (v < 0) {
 tg_dbg_raw("-");
 x = (unsigned int)(-v);
 } else {
 x = (unsigned int)v;
 }
 while (x > 0 && i < (int)sizeof(buf)) {
 buf[i++] = (char)('0' + (x % 10));
 x /= 10;
 }
 while (i > 0) {
 char ch = buf[--i];
 tg_sys_write(1, &ch, 1);
 }
}

static int tg_dbg_is_save_path(const char *path) {
 if (!path) {
 return 0;
 }
 return tg_dbg_contains_local(path, "doomsav") || tg_dbg_contains_local(path, ".dsg") || tg_dbg_contains_local(path, "save");
}

static void tg_dbg_save_op(const char *op, const char *path, int32_t rc) {
 if (!tg_dbg_is_save_path(path)) {
 return;
 }
 tg_dbg_raw("[doom-save] ");
 tg_dbg_raw(op);
 tg_dbg_raw(" path='");
 tg_dbg_raw(path ? path : "(null)");
 tg_dbg_raw("' rc=");
 tg_dbg_i32(rc);
 tg_dbg_raw("\n");
}

static void tg_dbg_save_rename(const char *oldpath, const char *newpath, int32_t rc) {
 if (!tg_dbg_is_save_path(oldpath) && !tg_dbg_is_save_path(newpath)) {
 return;
 }
 tg_dbg_raw("[doom-save] rename old='");
 tg_dbg_raw(oldpath ? oldpath : "(null)");
 tg_dbg_raw("' new='");
 tg_dbg_raw(newpath ? newpath : "(null)");
 tg_dbg_raw("' rc=");
 tg_dbg_i32(rc);
 tg_dbg_raw("\n");
}

static TgFile *to_file(FILE *f) {
 uintptr_t v = (uintptr_t)f;
 if (v <= 2) {
 return 0;
 }
 return (TgFile *)f;
}

static int streq(const char *a, const char *b) {
 while (*a && *b) {
 if (*a != *b) {
 return 0;
 }
 ++a;
 ++b;
 }
 return *a == 0 && *b == 0;
}

void *memcpy(void *dst, const void *src, size_t n) {
 unsigned char *d = (unsigned char *)dst;
 const unsigned char *s = (const unsigned char *)src;
 for (size_t i = 0; i < n; i++) {
 d[i] = s[i];
 }
 return dst;
}

void *memset(void *dst, int c, size_t n) {
 unsigned char *d = (unsigned char *)dst;
 for (size_t i = 0; i < n; i++) {
 d[i] = (unsigned char)c;
 }
 return dst;
}

void *memmove(void *dst, const void *src, size_t n) {
 unsigned char *d = (unsigned char *)dst;
 const unsigned char *s = (const unsigned char *)src;
 if (d < s) {
 for (size_t i = 0; i < n; i++) {
 d[i] = s[i];
 }
 } else if (d > s) {
 for (size_t i = n; i > 0; i--) {
 d[i - 1] = s[i - 1];
 }
 }
 return dst;
}

int memcmp(const void *a, const void *b, size_t n) {
 const unsigned char *x = (const unsigned char *)a;
 const unsigned char *y = (const unsigned char *)b;
 for (size_t i = 0; i < n; i++) {
 if (x[i] != y[i]) {
 return (int)x[i] - (int)y[i];
 }
 }
 return 0;
}

size_t strlen(const char *s) {
 size_t n = 0;
 while (s[n]) {
 n++;
 }
 return n;
}

char *strcpy(char *dst, const char *src) {
 size_t i = 0;
 while (src[i]) {
 dst[i] = src[i];
 i++;
 }
 dst[i] = 0;
 return dst;
}

char *strncpy(char *dst, const char *src, size_t n) {
 size_t i = 0;
 while (i < n && src[i]) {
 dst[i] = src[i];
 i++;
 }
 while (i < n) {
 dst[i++] = 0;
 }
 return dst;
}

int strcmp(const char *a, const char *b) {
 if (!a || !b) {
 if (a == b) {
 return 0;
 }
 return a ? 1 : -1;
 }
 while (*a && *b && *a == *b) {
 ++a;
 ++b;
 }
 return (unsigned char)*a - (unsigned char)*b;
}

int strncmp(const char *a, const char *b, size_t n) {
 if (!a || !b) {
 if (a == b) {
 return 0;
 }
 return a ? 1 : -1;
 }
 for (size_t i = 0; i < n; i++) {
 if (a[i] != b[i] || a[i] == 0 || b[i] == 0) {
 return (unsigned char)a[i] - (unsigned char)b[i];
 }
 }
 return 0;
}

static int tolower_ascii(int c) {
 if (c >= 'A' && c <= 'Z') {
 return c + ('a' - 'A');
 }
 return c;
}

static int toupper_ascii(int c) {
 if (c >= 'a' && c <= 'z') {
 return c - ('a' - 'A');
 }
 return c;
}

int strcasecmp(const char *a, const char *b) {
 if (!a || !b) {
 if (a == b) {
 return 0;
 }
 return a ? 1 : -1;
 }
 while (*a && *b) {
 int x = tolower_ascii((unsigned char)*a);
 int y = tolower_ascii((unsigned char)*b);
 if (x != y) {
 return x - y;
 }
 ++a;
 ++b;
 }
 return tolower_ascii((unsigned char)*a) - tolower_ascii((unsigned char)*b);
}

int strncasecmp(const char *a, const char *b, size_t n) {
 if (!a || !b) {
 if (a == b) {
 return 0;
 }
 return a ? 1 : -1;
 }
 for (size_t i = 0; i < n; i++) {
 int x = tolower_ascii((unsigned char)a[i]);
 int y = tolower_ascii((unsigned char)b[i]);
 if (x != y || a[i] == 0 || b[i] == 0) {
 return x - y;
 }
 }
 return 0;
}

char *strchr(const char *s, int c) {
 for (; *s; ++s) {
 if (*s == (char)c) {
 return (char *)s;
 }
 }
 return c == 0 ? (char *)s : (char *)0;
}

char *strrchr(const char *s, int c) {
 char *ret = 0;
 for (; *s; ++s) {
 if (*s == (char)c) {
 ret = (char *)s;
 }
 }
 if (c == 0) {
 return (char *)s;
 }
 return ret;
}

char *strstr(const char *haystack, const char *needle) {
 if (!*needle) {
 return (char *)haystack;
 }
 size_t n = strlen(needle);
 for (const char *p = haystack; *p; ++p) {
 if (strncmp(p, needle, n) == 0) {
 return (char *)p;
 }
 }
 return 0;
}

char *strdup(const char *s) {
 size_t n = strlen(s) + 1;
 char *p = (char *)tg_alloc((uintptr_t)n);
 if (!p) {
 return 0;
 }
 memcpy(p, s, n);
 return p;
}

void *malloc(size_t size) {
 size_t payload = size == 0 ? 1 : size;
 size_t total = payload + sizeof(size_t);
 size_t *raw = (size_t *)tg_alloc((uintptr_t)total);
 if (!raw) {
 return 0;
 }
 raw[0] = payload;
 return (void *)(raw + 1);
}

void free(void *ptr) {
 (void)ptr;
}

void *calloc(size_t n, size_t size) {
 size_t total = n * size;
 void *p = malloc(total);
 if (p) {
 memset(p, 0, total);
 }
 return p;
}

void *realloc(void *ptr, size_t size) {
 if (!ptr) {
 return malloc(size);
 }
 size_t payload = size == 0 ? 1 : size;
 size_t *old_raw = ((size_t *)ptr) - 1;
 size_t old_size = old_raw[0];
 void *p = malloc(payload);
 if (p && payload) {
 size_t copy_n = old_size < payload ? old_size : payload;
 memcpy(p, ptr, copy_n);
 }
 return p;
}

int abs(int x) {
 return x < 0 ? -x : x;
}

long strtol(const char *nptr, char **endptr, int base) {
 const char *s = nptr;
 while (*s == ' ' || *s == '\t' || *s == '\n' || *s == '\r') {
 s++;
 }
 int neg = 0;
 if (*s == '-' || *s == '+') {
 neg = (*s == '-');
 s++;
 }
 if (base == 0) {
 base = 10;
 if (s[0] == '0' && (s[1] == 'x' || s[1] == 'X')) {
 base = 16;
 s += 2;
 }
 }
 long v = 0;
 while (*s) {
 int d;
 if (*s >= '0' && *s <= '9') {
 d = *s - '0';
 } else if (*s >= 'a' && *s <= 'f') {
 d = *s - 'a' + 10;
 } else if (*s >= 'A' && *s <= 'F') {
 d = *s - 'A' + 10;
 } else {
 break;
 }
 if (d >= base) {
 break;
 }
 v = v * base + d;
 s++;
 }
 if (endptr) {
 *endptr = (char *)s;
 }
 return neg ? -v : v;
}

int atoi(const char *s) {
 return (int)strtol(s, 0, 10);
}

long atol(const char *s) {
 return strtol(s, 0, 10);
}

double strtod(const char *nptr, char **endptr) {
 long v = strtol(nptr, endptr, 10);
 return (double)v;
}

static TgFile *alloc_file(void) {
 for (int i = 0; i < TG_MAX_FILES; i++) {
 if (!g_files[i].used) {
 g_files[i].used = 1;
 g_files[i].mode_write = 0;
 g_files[i].fd = -1;
 g_files[i].err = 0;
 g_files[i].eof = 0;
 g_files[i].data = 0;
 g_files[i].size = 0;
 g_files[i].pos = 0;
 g_files[i].path[0] = 0;
 g_files[i].write_buf_len = 0;
 return &g_files[i];
 }
 }
 return 0;
}

FILE *fopen(const char *path, const char *mode) {
 if (!path || !mode) {
 return 0;
 }
 TgFile *f = alloc_file();
 if (!f) {
 return 0;
 }

 if (mode[0] == 'r') {
 if (g_read_cache.valid && strcmp(path, g_read_cache.path) == 0) {
 f->data = g_read_cache.data;
 f->size = g_read_cache.size;
 f->pos = 0;
 f->mode_write = 0;
 f->fd = -1;
 return (FILE *)f;
 }

 int fd = tg_sys_open(path, TG_O_RDONLY);
 tg_dbg_save_op("open-r", path, fd);
 if (fd < 0) {
 f->used = 0;
 return 0;
 }

 // 第一遍：只探测文件大小（避免 free() 无效导致扩容泄漏）。
 size_t size = 0;
 unsigned char probe[4096];
 for (;;) {
 int32_t n = tg_sys_read(fd, probe, sizeof(probe));
 if (n < 0) {
 tg_sys_close(fd);
 f->used = 0;
 return 0;
 }
 if (n == 0) {
 break;
 }
 size += (size_t)n;
 if (size > TG_MAX_FILE_SIZE) {
 tg_sys_close(fd);
 f->used = 0;
 return 0;
 }
 }
 tg_sys_close(fd);

 // 第二遍：按最终大小一次分配并完整读入。
 fd = tg_sys_open(path, TG_O_RDONLY);
 if (fd < 0) {
 f->used = 0;
 return 0;
 }

 unsigned char *buf = (unsigned char *)malloc(size == 0 ? 1 : size);
 if (!buf) {
 tg_sys_close(fd);
 f->used = 0;
 return 0;
 }

 size_t got = 0;
 while (got < size) {
 int32_t n = tg_sys_read(fd, buf + got, (uintptr_t)(size - got));
 if (n <= 0) {
 break;
 }
 got += (size_t)n;
 }
 tg_sys_close(fd);
 if (tg_dbg_is_save_path(path)) {
 tg_dbg_raw("[doom-save] open-r-close path='");
 tg_dbg_raw(path);
 tg_dbg_raw("' rc=0 bytes=");
 tg_dbg_i32((int32_t)got);
 tg_dbg_raw("\n");
 }

 f->data = buf;
 f->size = got;
 f->pos = 0;
 f->mode_write = 0;
 f->fd = -1;
 strncpy(f->path, path, TG_PATH_MAX - 1);
 f->path[TG_PATH_MAX - 1] = 0;

 g_read_cache.valid = 1;
 strncpy(g_read_cache.path, path, TG_PATH_MAX - 1);
 g_read_cache.path[TG_PATH_MAX - 1] = 0;
 g_read_cache.data = buf;
 g_read_cache.size = got;

 return (FILE *)f;
 }

 if (mode[0] == 'w') {
 int fd = tg_sys_open(path, TG_O_WRONLY | TG_O_CREATE | TG_O_TRUNC);
 tg_dbg_save_op("open-w", path, fd);
 if (fd < 0) {
 f->used = 0;
 return 0;
 }
 f->mode_write = 1;
 f->fd = fd;
 f->data = 0;
 f->size = 0;
 f->pos = 0;
 strncpy(f->path, path, TG_PATH_MAX - 1);
 f->path[TG_PATH_MAX - 1] = 0;
 return (FILE *)f;
 }

 f->used = 0;
 return 0;
}

static int flush_write_buffer(TgFile *f) {
 if (!f || f->write_buf_len == 0 || f->fd < 0) {
 return 0;
 }
 size_t written = 0;
 while (written < f->write_buf_len) {
 int32_t n = tg_sys_write(f->fd, f->write_buf + written, (uintptr_t)(f->write_buf_len - written));
 if (n <= 0) {
 f->err = 1;
 return -1;
 }
 written += (size_t)n;
 }
 f->write_buf_len = 0;
 return 0;
}

int fclose(FILE *stream) {
 TgFile *f = to_file(stream);
 if (!f) {
 return 0;
 }
 if (f->mode_write && f->fd >= 0) {
 if (flush_write_buffer(f) < 0) {
 // continue to close even on flush failure
 }
 int32_t rc = tg_sys_close(f->fd);
 if (tg_dbg_is_save_path(f->path)) {
 tg_dbg_raw("[doom-save] close path='");
 tg_dbg_raw(f->path);
 tg_dbg_raw("' bytes=");
 tg_dbg_i32((int32_t)f->pos);
 tg_dbg_raw(" rc=");
 tg_dbg_i32(rc);
 tg_dbg_raw("\n");
 }
 f->fd = -1;
 }
 f->used = 0;
 return 0;
}

size_t fread(void *ptr, size_t size, size_t nmemb, FILE *stream) {
 TgFile *f = to_file(stream);
 if (!f || !ptr || size == 0 || nmemb == 0 || f->mode_write) {
 return 0;
 }
 size_t total = size * nmemb;
 size_t remain = f->size > f->pos ? f->size - f->pos : 0;
 size_t bytes = total < remain ? total : remain;
 memcpy(ptr, f->data + f->pos, bytes);
 f->pos += bytes;
 if (bytes < total) {
 f->eof = 1;
 }
 return bytes / size;
}

size_t fwrite(const void *ptr, size_t size, size_t nmemb, FILE *stream) {
 TgFile *f = to_file(stream);
 if (!f || !f->mode_write || f->fd < 0 || !ptr || size == 0 || nmemb == 0) {
 return 0;
 }
 size_t total = size * nmemb;
 if (f->pos == 0 && tg_dbg_is_save_path(f->path)) {
 tg_dbg_raw("[doom-save] fwrite-begin path='");
 tg_dbg_raw(f->path);
 tg_dbg_raw("' total=");
 tg_dbg_i32((int32_t)total);
 tg_dbg_raw("\n");
 }
 size_t old_pos = f->pos;
 size_t offset = 0;
 while (offset < total) {
 size_t chunk = total - offset;
 if (chunk > sizeof(f->write_buf) - f->write_buf_len) {
 chunk = sizeof(f->write_buf) - f->write_buf_len;
 }
 memcpy(f->write_buf + f->write_buf_len, (const unsigned char *)ptr + offset, chunk);
 f->write_buf_len += chunk;
 offset += chunk;

 if (f->write_buf_len == sizeof(f->write_buf)) {
 if (flush_write_buffer(f) < 0) {
 if (tg_dbg_is_save_path(f->path)) {
 tg_dbg_raw("[doom-save] fwrite-error flush path='");
 tg_dbg_raw(f->path);
 tg_dbg_raw("' wrote="); tg_dbg_i32((int32_t)f->pos);
 tg_dbg_raw(" total="); tg_dbg_i32((int32_t)total);
 tg_dbg_raw("\n");
 }
 break;
 }
 }

 if (tg_dbg_is_save_path(f->path) && (f->write_buf_len == 0 || f->write_buf_len % 1024 == 0)) {
 tg_dbg_raw("[doom-save] fwrite-buffer path='");
 tg_dbg_raw(f->path);
 tg_dbg_raw("' buf="); tg_dbg_i32((int32_t)f->write_buf_len);
 tg_dbg_raw(" total="); tg_dbg_i32((int32_t)total);
 tg_dbg_raw("\n");
 }
 }

 f->pos += (offset / size) * size;

 if (tg_dbg_is_save_path(f->path)) {
 size_t old_bucket = old_pos / (64 * 1024);
 size_t new_bucket = f->pos / (64 * 1024);
 if (new_bucket > old_bucket) {
 tg_dbg_raw("[doom-save] fwrite-progress64k path='");
 tg_dbg_raw(f->path);
 tg_dbg_raw("' pos=");
 tg_dbg_i32((int32_t)f->pos);
 tg_dbg_raw(" buf=");
 tg_dbg_i32((int32_t)f->write_buf_len);
 tg_dbg_raw("\n");
 }
 }
 return offset / size;
}

int fseek(FILE *stream, long offset, int whence) {
 TgFile *f = to_file(stream);
 if (!f) {
 return -1;
 }
 long base = 0;
 if (whence == SEEK_SET) {
 base = 0;
 } else if (whence == SEEK_CUR) {
 base = (long)f->pos;
 } else if (whence == SEEK_END) {
 base = (long)f->size;
 } else {
 return -1;
 }
 long pos = base + offset;
 if (pos < 0) {
 pos = 0;
 }
 if ((size_t)pos > f->size) {
 pos = (long)f->size;
 }
 f->pos = (size_t)pos;
 f->eof = 0;
 return 0;
}

long ftell(FILE *stream) {
 TgFile *f = to_file(stream);
 if (!f) {
 return -1;
 }
 return (long)f->pos;
}

int fflush(FILE *stream) {
 if (!stream) {
 for (int i = 0; i < TG_MAX_FILES; i++) {
 TgFile *f = &g_files[i];
 if (f->used && f->mode_write && f->fd >= 0) {
 if (flush_write_buffer(f) < 0) {
 return -1;
 }
 }
 }
 return 0;
 }

 TgFile *f = to_file(stream);
 if (!f) {
 return 0;
 }
 if (f->mode_write && f->fd >= 0) {
 return flush_write_buffer(f) < 0 ? -1 : 0;
 }
 return 0;
}

int feof(FILE *stream) {
 TgFile *f = to_file(stream);
 if (!f) {
 return 1;
 }
 return f->eof;
}

int ferror(FILE *stream) {
 TgFile *f = to_file(stream);
 if (!f) {
 return 1;
 }
 return f->err;
}

int remove(const char *path) {
 if (!path) {
 return -1;
 }
 int32_t rc = tg_sys_unlink(path);
 tg_dbg_save_op("remove", path, rc);
 return rc >= 0 ? 0 : -1;
}

int rename(const char *oldpath, const char *newpath) {
 if (!oldpath || !newpath) {
 tg_dbg_save_rename(oldpath, newpath, -1);
 return -1;
 }
 if (strcmp(oldpath, newpath) == 0) {
 tg_dbg_save_rename(oldpath, newpath, 0);
 return 0;
 }

 int src = tg_sys_open(oldpath, TG_O_RDONLY);
 if (src < 0) {
 tg_dbg_save_rename(oldpath, newpath, -2);
 return -1;
 }

 int dst = tg_sys_open(newpath, TG_O_WRONLY | TG_O_CREATE | TG_O_TRUNC);
 if (dst < 0) {
 tg_sys_close(src);
 tg_dbg_save_rename(oldpath, newpath, -3);
 return -1;
 }

 unsigned char buf[4096];
 size_t copied_total = 0;
 int ok = 1;
 for (;;) {
 int32_t n = tg_sys_read(src, buf, sizeof(buf));
 if (n < 0) {
 ok = 0;
 break;
 }
 if (n == 0) {
 break;
 }
 copied_total += (size_t)n;
 if (copied_total > TG_MAX_FILE_SIZE) {
 ok = 0;
 break;
 }
 size_t wrote = 0;
 while (wrote < (size_t)n) {
 int32_t m = tg_sys_write(dst, buf + wrote, (uintptr_t)((size_t)n - wrote));
 if (m <= 0) {
 ok = 0;
 break;
 }
 wrote += (size_t)m;
 }
 if (!ok) {
 break;
 }
 }

 tg_sys_close(dst);
 tg_sys_close(src);

 if (!ok) {
 tg_dbg_save_rename(oldpath, newpath, -4);
 return -1;
 }

 int32_t del_rc = tg_sys_unlink(oldpath);
 if (del_rc < 0) {
 tg_dbg_save_rename(oldpath, newpath, -5);
 return -1;
 }

 tg_dbg_save_rename(oldpath, newpath, 0);
 return 0;
}

int mkdir(const char *path, unsigned int mode) {
 (void)path;
 (void)mode;
 return -1;
}

int puts(const char *s) {
 if (!s) {
 return 0;
 }
 tg_sys_write(1, s, (uintptr_t)strlen(s));
 tg_sys_write(1, "\n", 1);
 return 0;
}

int putchar(int c) {
 char ch = (char)c;
 tg_sys_write(1, &ch, 1);
 return c;
}

int putc(int c, FILE *stream) {
 char ch = (char)c;
 if ((uintptr_t)stream == 2) {
 tg_sys_write(2, &ch, 1);
 return c;
 }
 if ((uintptr_t)stream <= 1) {
 tg_sys_write(1, &ch, 1);
 return c;
 }
 TgFile *f = to_file(stream);
 if (!f || !f->mode_write || f->fd < 0) {
 return -1;
 }
 int32_t n = tg_sys_write(f->fd, &ch, 1);
 if (n <= 0) {
 f->err = 1;
 return -1;
 }
 f->pos += 1;
 return c;
}

static int is_space(int ch) {
 return ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r' || ch == '\f' || ch == '\v';
}

static int append_ch(char *dst, size_t size, size_t *pos, char ch) {
 if (dst && *pos + 1 < size) {
 dst[*pos] = ch;
 }
 (*pos)++;
 return 1;
}

static int append_str(char *dst, size_t size, size_t *pos, const char *s) {
 int count = 0;
 if (!s) {
 s = "(null)";
 }
 while (*s) {
 count += append_ch(dst, size, pos, *s++);
 }
 return count;
}

static int append_uint_base(char *dst, size_t size, size_t *pos, unsigned long long value,
 unsigned base, int upper) {
 char tmp[32];
 int n = 0;
 const char *digits = upper ? "0123456789ABCDEF" : "0123456789abcdef";
 if (base < 2 || base > 16) {
 return 0;
 }
 if (value == 0) {
 return append_ch(dst, size, pos, '0');
 }
 while (value > 0 && n < (int)sizeof(tmp)) {
 tmp[n++] = digits[value % base];
 value /= base;
 }
 int count = 0;
 while (n > 0) {
 count += append_ch(dst, size, pos, tmp[--n]);
 }
 return count;
}

static int append_uint_base_prec(char *dst, size_t size, size_t *pos, unsigned long long value,
 unsigned base, int upper, int min_digits) {
 char tmp[32];
 int n = 0;
 const char *digits = upper ? "0123456789ABCDEF" : "0123456789abcdef";
 if (base < 2 || base > 16) {
 return 0;
 }
 if (value == 0) {
 if (min_digits == 0) {
 return 0;
 }
 tmp[n++] = '0';
 } else {
 while (value > 0 && n < (int)sizeof(tmp)) {
 tmp[n++] = digits[value % base];
 value /= base;
 }
 }
 while (n < min_digits && n < (int)sizeof(tmp)) {
 tmp[n++] = '0';
 }
 int count = 0;
 while (n > 0) {
 count += append_ch(dst, size, pos, tmp[--n]);
 }
 return count;
}

static int tg_vsnprintf(char *str, size_t size, const char *fmt, va_list ap) {
 size_t pos = 0;
 // 防御性检查：fmt 指针不能为 NULL
 if (!fmt) {
 if (str && size > 0) {
 str[0] = 0;
 }
 return 0;
 }

 for (size_t i = 0; fmt[i] != 0; i++) {
 if (fmt[i] != '%') {
 append_ch(str, size, &pos, fmt[i]);
 continue;
 }

 i++;
 if (fmt[i] == 0) {
 break;
 }
 if (fmt[i] == '%') {
 append_ch(str, size, &pos, '%');
 continue;
 }

 while (fmt[i] == '-' || fmt[i] == '+' || fmt[i] == ' ' || fmt[i] == '#' || fmt[i] == '0') {
 i++;
 }
 while (fmt[i] >= '0' && fmt[i] <= '9') {
 i++;
 }
 int precision = -1;
 if (fmt[i] == '.') {
 i++;
 precision = 0;
 while (fmt[i] >= '0' && fmt[i] <= '9') {
 precision = precision * 10 + (fmt[i] - '0');
 i++;
 }
 }

 int length = 0;
 if (fmt[i] == 'h') {
 length = 1;
 i++;
 if (fmt[i] == 'h') {
 length = 2;
 i++;
 }
 } else if (fmt[i] == 'l') {
 length = 3;
 i++;
 if (fmt[i] == 'l') {
 length = 4;
 i++;
 }
 } else if (fmt[i] == 'z') {
 length = 5;
 i++;
 }

 char spec = fmt[i];
 switch (spec) {
 case 'c': {
 int v = va_arg(ap, int);
 append_ch(str, size, &pos, (char)v);
 break;
 }
 case 's': {
 const char *s = va_arg(ap, const char *);
 append_str(str, size, &pos, s);
 break;
 }
 case 'd':
 case 'i': {
 long long v;
 if (length == 4) {
 v = va_arg(ap, long long);
 } else if (length == 3) {
 v = va_arg(ap, long);
 } else {
 v = va_arg(ap, int);
 }
 unsigned long long uv;
 if (v < 0) {
 append_ch(str, size, &pos, '-');
 uv = (unsigned long long)(-(v + 1)) + 1;
 } else {
 uv = (unsigned long long)v;
 }
 append_uint_base_prec(str, size, &pos, uv, 10, 0, precision >= 0 ? precision : 1);
 break;
 }
 case 'u':
 case 'x':
 case 'X':
 case 'o': {
 unsigned base = (spec == 'u') ? 10 : (spec == 'o' ? 8 : 16);
 unsigned long long v;
 if (length == 5) {
 v = (unsigned long long)va_arg(ap, size_t);
 } else if (length == 4) {
 v = va_arg(ap, unsigned long long);
 } else if (length == 3) {
 v = va_arg(ap, unsigned long);
 } else {
 v = va_arg(ap, unsigned int);
 }
 append_uint_base_prec(str, size, &pos, v, base, spec == 'X', precision >= 0 ? precision : 1);
 break;
 }
 case 'p': {
 uintptr_t v = (uintptr_t)va_arg(ap, void *);
 append_str(str, size, &pos, "0x");
 append_uint_base(str, size, &pos, (unsigned long long)v, 16, 0);
 break;
 }
 default:
 append_ch(str, size, &pos, '%');
 append_ch(str, size, &pos, spec);
 break;
 }
 }

 if (str && size > 0) {
 size_t end = pos < (size - 1) ? pos : (size - 1);
 str[end] = 0;
 }
 return (int)pos;
}

int vprintf(const char *fmt, va_list ap) {
    // 如果格式字符串为 NULL，直接返回（防止 newlib printf 崩溃）
    if (!fmt) {
        return 0;
    }
    // NOTE: Using a static buffer to avoid stack overflow on small user stacks (only 2 pages = 8KB).
    // This is safe because printf is single-threaded and non-reentrant.
    static char buf[1024];
    va_list ap_copy;
    va_copy(ap_copy, ap);
    int n = tg_vsnprintf(buf, sizeof(buf), fmt, ap_copy);
    va_end(ap_copy);
    if (n > 0) {
        size_t out = (size_t)n;
        if (out >= sizeof(buf)) {
            out = sizeof(buf) - 1;
        }
        tg_sys_write(1, buf, (uintptr_t)out);
    }
    return n;
}

int printf(const char *fmt, ...) {
    if (!fmt) return 0;
    va_list ap;
    va_start(ap, fmt);
    int ret = vprintf(fmt, ap);
    va_end(ap);
    return ret;
}

int fprintf(FILE *stream, const char *fmt, ...) {
    if (!fmt) return 0;
    va_list ap;
    va_start(ap, fmt);
    char buf[1024];
    int n = tg_vsnprintf(buf, sizeof(buf), fmt, ap);
    if (n > 0) {
        size_t out = (size_t)n;
        if (out >= sizeof(buf)) {
            out = sizeof(buf) - 1;
        }
        if ((uintptr_t)stream == 2) {
            tg_sys_write(2, buf, (uintptr_t)out);
        } else if ((uintptr_t)stream <= 1) {
            tg_sys_write(1, buf, (uintptr_t)out);
        } else {
            TgFile *f = to_file(stream);
            if (f && f->mode_write && f->fd >= 0) {
                size_t wrote = 0;
                while (wrote < out) {
                    int32_t w = tg_sys_write(f->fd, buf + wrote, (uintptr_t)(out - wrote));
                    if (w <= 0) {
                        f->err = 1;
                        break;
                    }
                    wrote += (size_t)w;
                }
                f->pos += wrote;
            }
        }
    }
    va_end(ap);
    return n;
}

int snprintf(char *str, size_t size, const char *fmt, ...) {
    if (!fmt || !str || size == 0) {
        if (str && size > 0) str[0] = 0;
        return 0;
    }
    va_list ap;
    va_start(ap, fmt);
    int ret = tg_vsnprintf(str, size, fmt, ap);
    va_end(ap);
    return ret;
}

int vsnprintf(char *str, size_t size, const char *fmt, va_list ap) {
 return tg_vsnprintf(str, size, fmt, ap);
}

int __printf_chk(int flag, const char *fmt, ...) {
 (void)flag;
 va_list ap;
 va_start(ap, fmt);
 int ret = vprintf(fmt, ap);
 va_end(ap);
 return ret;
}

int __fprintf_chk(FILE *stream, int flag, const char *fmt, ...) {
 (void)flag;
 va_list ap;
 va_start(ap, fmt);
 char buf[1024];
 int n = tg_vsnprintf(buf, sizeof(buf), fmt, ap);
 if (n > 0) {
 size_t out = (size_t)n;
 if (out >= sizeof(buf)) {
 out = sizeof(buf) - 1;
 }
 if ((uintptr_t)stream == 2) {
 tg_sys_write(2, buf, (uintptr_t)out);
 } else if ((uintptr_t)stream <= 1) {
 tg_sys_write(1, buf, (uintptr_t)out);
 } else {
 TgFile *f = to_file(stream);
 if (f && f->mode_write && f->fd >= 0) {
 size_t wrote = 0;
 while (wrote < out) {
 int32_t w = tg_sys_write(f->fd, buf + wrote, (uintptr_t)(out - wrote));
 if (w <= 0) {
 f->err = 1;
 break;
 }
 wrote += (size_t)w;
 }
 f->pos += wrote;
 }
 }
 }
 va_end(ap);
 return n;
}

int __vfprintf_chk(FILE *stream, int flag, const char *fmt, va_list ap) {
 (void)flag;
 char buf[1024];
 int n = tg_vsnprintf(buf, sizeof(buf), fmt, ap);
 if (n > 0) {
 size_t out = (size_t)n;
 if (out >= sizeof(buf)) {
 out = sizeof(buf) - 1;
 }
 if ((uintptr_t)stream == 2) {
 tg_sys_write(2, buf, (uintptr_t)out);
 } else if ((uintptr_t)stream <= 1) {
 tg_sys_write(1, buf, (uintptr_t)out);
 } else {
 TgFile *f = to_file(stream);
 if (f && f->mode_write && f->fd >= 0) {
 size_t wrote = 0;
 while (wrote < out) {
 int32_t w = tg_sys_write(f->fd, buf + wrote, (uintptr_t)(out - wrote));
 if (w <= 0) {
 f->err = 1;
 break;
 }
 wrote += (size_t)w;
 }
 f->pos += wrote;
 }
 }
 }
 return n;
}

int __snprintf_chk(char *s, size_t maxlen, int flag, size_t slen, const char *fmt, ...) {
 (void)flag;
 (void)slen;
 va_list ap;
 va_start(ap, fmt);
 int ret = vsnprintf(s, maxlen, fmt, ap);
 va_end(ap);
 return ret;
}

int __vsnprintf_chk(char *s, size_t maxlen, int flag, size_t slen, const char *fmt, va_list ap) {
 (void)flag;
 (void)slen;
 return vsnprintf(s, maxlen, fmt, ap);
}

int __isoc99_sscanf(const char *s, const char *fmt, ...) {
 if (!s || !fmt) {
 return 0;
 }
 va_list ap;
 va_start(ap, fmt);

 while (is_space((unsigned char)*fmt)) {
 ++fmt;
 }
 while (is_space((unsigned char)*s)) {
 ++s;
 }

 if (*fmt != '%') {
 va_end(ap);
 return 0;
 }
 ++fmt;

 int base = 10;
 int auto_base = 0;
 if (*fmt == 'x' || *fmt == 'X') {
 base = 16;
 } else if (*fmt == 'o') {
 base = 8;
 } else if (*fmt == 'd' || *fmt == 'u') {
 base = 10;
 } else if (*fmt == 'i') {
 auto_base = 1;
 } else {
 va_end(ap);
 return 0;
 }

 int neg = 0;
 if (*s == '+' || *s == '-') {
 neg = (*s == '-');
 ++s;
 }

 if (auto_base) {
 base = 10;
 if (s[0] == '0' && (s[1] == 'x' || s[1] == 'X')) {
 base = 16;
 s += 2;
 }
 }

 unsigned long value = 0;
 int digits = 0;
 while (*s) {
 int d;
 if (*s >= '0' && *s <= '9') {
 d = *s - '0';
 } else if (*s >= 'a' && *s <= 'f') {
 d = *s - 'a' + 10;
 } else if (*s >= 'A' && *s <= 'F') {
 d = *s - 'A' + 10;
 } else {
 break;
 }
 if (d >= base) {
 break;
 }
 value = value * (unsigned long)base + (unsigned long)d;
 ++s;
 ++digits;
 }

 if (digits == 0) {
 va_end(ap);
 return 0;
 }

 int *out = va_arg(ap, int *);
 if (out) {
 long signed_value = neg ? -(long)value : (long)value;
 *out = (int)signed_value;
 }

 va_end(ap);
 return 1;
}

int *__errno_location(void) {
 static int e = 0;
 return &e;
}

int system(const char *cmd) {
 (void)cmd;
 return -1;
}

const int **__ctype_toupper_loc(void) {
 static int table_data[256];
 static const int *table = table_data;
 static int inited = 0;
 if (!inited) {
 for (int i = 0; i < 256; i++) {
 table_data[i] = toupper_ascii(i);
 }
 inited = 1;
 }
 return &table;
}

char *getenv(const char *name) {
 (void)name;
 return 0;
}

unsigned int sleep(unsigned int seconds) {
 (void)seconds;
 return 0;
}

void exit(int code) {
 tg_dbg_raw("[doom] libc exit called code=");
 tg_dbg_i32(code);
 tg_dbg_raw("\n");

#if defined(__riscv)
 register int a0 asm("a0") = code;
 register int a7 asm("a7") = 93; // SYS_exit
 asm volatile("ecall" : "+r"(a0) : "r"(a7) : "memory");
 (void)a0;
#endif

 for (;;) {
 asm volatile("wfi");
 }
}

// 网络相关存根（Doom 网络游戏支持）
int drone = 0;
int net_client_connected = 0;

// OPL 相关
int opl_io_port = 0;
