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
