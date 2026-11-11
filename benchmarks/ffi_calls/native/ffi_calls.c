/* The C side of the FFI call-overhead benchmark. Each function does almost
 * nothing, so a caller's time is the cost of crossing into C. */
#include <stdint.h>
#include <stdlib.h>

int64_t fc_add(int64_t a, int64_t b) { return a + b; }
int32_t fc_narrow(int32_t a, uint8_t b) { return a + b; }
double fc_scale(double value, float factor) { return value * factor; }
int64_t fc_six(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e, int64_t f) {
    return a + b + c + d + e + f;
}
uint64_t fc_text(const uint8_t *text, uint64_t length) {
    return length == 0 ? 0 : text[0] + length;
}
uint64_t fc_bytes(const uint8_t *bytes, uint64_t length) {
    return length == 0 ? 0 : bytes[length - 1] + length;
}
void fc_fill(uint8_t *bytes, uint64_t length) {
    if (length != 0) bytes[0]++;
}
uint64_t fc_cstr(const char *text) { return (uint64_t)text[0]; }
const char *fc_name(void) { return "hello, world"; }
int32_t fc_out(int64_t value, int64_t *out) {
    *out += value;
    return 0;
}
double fc_floats(const double *values, int64_t count) { return values[count - 1]; }
void fc_bump(double *values, int64_t count) { values[count - 1] += 1.0; }

typedef struct { double x, y; } Vec2;
typedef struct { int64_t a, b, c; } Triple;
Vec2 fc_vec2_add(Vec2 a, Vec2 b) { Vec2 r = {a.x + b.x, a.y + b.y}; return r; }
Triple fc_triple_next(Triple t) { Triple r = {t.b, t.c, t.a + 1}; return r; }
void fc_vec2_move(Vec2 *v, double by) { v->x += by; v->y -= by; }
