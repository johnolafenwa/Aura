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
