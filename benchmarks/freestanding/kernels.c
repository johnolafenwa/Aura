// The routines of `kernels.au` in C. Unsigned arithmetic wraps and signed
// overflow is undefined, so this code holds no overflow or divisor check.
#include <stdint.h>

typedef struct { double x, y, vx, vy; } Body;

static Body step(Body body, double dt) {
    double r2 = body.x * body.x + body.y * body.y;
    double inverse = 1.0 / (r2 * __builtin_sqrt(r2));
    double vx = body.vx - body.x * inverse * dt;
    double vy = body.vy - body.y * inverse * dt;
    return (Body){body.x + vx * dt, body.y + vy * dt, vx, vy};
}

uint32_t c_crc32(uint32_t count) {
    uint32_t crc = 0xffffffffu;
    for (uint32_t i = 0; i < count; i++) {
        crc ^= i & 255;
        for (int bit = 0; bit < 8; bit++)
            crc = (crc & 1) ? (crc >> 1) ^ 0xedb88320u : crc >> 1;
    }
    return crc ^ 0xffffffffu;
}

uint64_t c_collatz(uint64_t limit) {
    uint64_t total = 0;
    for (uint64_t start = 1; start < limit; start++)
        for (uint64_t n = start; n != 1; total++)
            n = (n % 2 == 0) ? n / 2 : 3 * n + 1;
    return total;
}

uint64_t c_isqrt_sum(uint64_t limit) {
    uint64_t total = 0;
    for (uint64_t value = 1; value < limit; value++) {
        uint64_t guess = value, next = (guess + 1) / 2;
        while (next < guess) {
            guess = next;
            next = (guess + value / guess) / 2;
        }
        total += guess;
    }
    return total;
}

int64_t c_gcd_sum(int64_t limit) {
    int64_t total = 0;
    for (int64_t a = 1; a < limit; a++) {
        int64_t x = a, y = limit - a;
        while (y != 0) {
            int64_t rest = x % y;
            x = y;
            y = rest;
        }
        total += x;
    }
    return total;
}

double c_orbit(int64_t steps) {
    Body body = {1.0, 0.0, 0.0, 1.0};
    for (int64_t i = 0; i < steps; i++)
        body = step(body, 0.001);
    return body.x * body.x + body.y * body.y;
}
