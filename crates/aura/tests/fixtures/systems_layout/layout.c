#include <stdint.h>
#include <stddef.h>

struct __attribute__((packed, aligned(8))) Header {
    uint8_t kind;
    uint32_t serial;
    uint16_t count : 5;
    uint16_t mode : 3;
};

uint64_t header_size(void) { return sizeof(struct Header); }
uint64_t header_alignment(void) { return _Alignof(struct Header); }
uint64_t serial_offset(void) { return offsetof(struct Header, serial); }
uint8_t update_header(struct Header *h) {
    uint8_t ok = h->kind == 7 && h->serial == 123456 && h->count == 3 && h->mode == 6;
    h->count = 17;
    h->mode = 5;
    return ok;
}

struct __attribute__((packed)) Small {
    uint8_t tag;
    uint32_t serial;
};

struct Small make_small(struct Small x, uint32_t delta) {
    x.tag += 1;
    x.serial += delta;
    return x;
}

struct __attribute__((aligned(32))) Line {
    uint32_t value;
};

uint8_t aligned_line(struct Line *line) {
    return (uintptr_t)line % _Alignof(struct Line) == 0 && line->value == 17;
}

struct Packet {
    struct Small rows[2];
    struct Line lines[2];
};

uint8_t check_packet(struct Packet *p) {
    uint8_t ok = p->rows[0].serial == 9 && p->rows[1].serial == 7
        && p->lines[0].value == 17 && p->lines[1].value == 19;
    p->rows[1].serial = 55;
    p->lines[1].value = 77;
    return ok;
}

struct Line bump_line(uint32_t before, struct Line line, uint32_t after) {
    line.value += before + after;
    return line;
}
