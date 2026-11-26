// Storage with a fixed address and a bridge from an Aura string literal
// to an address and byte count. Aura fills every interrupt gate and
// performs every port access.
#include <stdint.h>
#include <stddef.h>

extern void breakpoint_handler(void);
extern void timer_handler(void);
extern void serial_bytes(uint64_t address, uint64_t length);

static _Alignas(16) unsigned char idt[256 * 16];
static unsigned char idtr[10];
static unsigned char handled[2];

uintptr_t board_idt(void) { return (uintptr_t)idt; }
uintptr_t board_idtr(void) { return (uintptr_t)idtr; }
uintptr_t board_events(void) { return (uintptr_t)handled; }
uintptr_t board_breakpoint(void) { return (uintptr_t)breakpoint_handler; }
uintptr_t board_timer(void) { return (uintptr_t)timer_handler; }

void serial_text(const uint8_t *text, size_t length) {
    serial_bytes((uintptr_t)text, length);
}
