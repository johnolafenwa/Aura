// Times one build of the five routines. `P` is the symbol prefix.
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#define NAME2(prefix, name) prefix##_##name
#define NAME(prefix, name) NAME2(prefix, name)

uint32_t NAME(P, crc32)(uint32_t);
uint64_t NAME(P, collatz)(uint64_t);
uint64_t NAME(P, isqrt_sum)(uint64_t);
int64_t NAME(P, gcd_sum)(int64_t);
double NAME(P, orbit)(int64_t);

void aura_trap(uint32_t code, const char *file, uint64_t length, uint32_t line, uint32_t column) {
    fprintf(stderr, "trap %u at %.*s:%u:%u\n", code, (int)length, file, line, column);
    exit(1);
}

// Rust's prebuilt `core` names this routine and never calls it.
void rust_eh_personality(void) {}

static double now(void) {
    struct timespec time;
    clock_gettime(CLOCK_MONOTONIC, &time);
    return time.tv_sec * 1e3 + time.tv_nsec / 1e6;
}

#define TIME(label, format, call)                                  \
    do {                                                           \
        double best = 1e30;                                        \
        for (int run = 0; run < 9; run++) {                        \
            double start = now();                                  \
            result_##label = call;                                 \
            double took = now() - start;                           \
            if (took < best) best = took;                          \
        }                                                          \
        printf("%-10s %8.1f ms  " format "\n", #label, best, result_##label); \
    } while (0)

int main(void) {
    uint32_t result_crc32;
    uint64_t result_collatz, result_isqrt_sum;
    int64_t result_gcd_sum;
    double result_orbit;
    TIME(crc32, "%" PRIu32, NAME(P, crc32)(20000000));
    TIME(collatz, "%" PRIu64, NAME(P, collatz)(1000000));
    TIME(isqrt_sum, "%" PRIu64, NAME(P, isqrt_sum)(5000000));
    TIME(gcd_sum, "%" PRId64, NAME(P, gcd_sum)(3000000));
    TIME(orbit, "%.12f", NAME(P, orbit)(50000000));
    return 0;
}
