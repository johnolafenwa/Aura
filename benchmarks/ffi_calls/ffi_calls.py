"""The Python side of the FFI call-overhead benchmark.

Runs the same calls as `src/main.au` through ctypes, and through cffi when it
is installed. Usage: `python3 ffi_calls.py <path to the library>`.
"""
import ctypes
import sys
import time

CALLS = 2_000_000


def report(name, started):
    print(f'{name} {(time.perf_counter_ns() - started) / CALLS}')


def run(prefix, lib, text, data, buffer):
    total = 0
    started = time.perf_counter_ns()
    for i in range(CALLS):
        total = lib.fc_add(total, i)
    report(f'{prefix}add', started)

    narrow = 0
    started = time.perf_counter_ns()
    for _ in range(CALLS):
        narrow = lib.fc_narrow(narrow % 1000, 3)
    report(f'{prefix}narrow', started)

    scaled = 1.0
    started = time.perf_counter_ns()
    for _ in range(CALLS):
        scaled = lib.fc_scale(scaled, 1.0)
    report(f'{prefix}scale', started)

    six = 0
    started = time.perf_counter_ns()
    for i in range(CALLS):
        six = lib.fc_six(six, i, 1, 2, 3, 4)
    report(f'{prefix}six', started)

    text_total = 0
    started = time.perf_counter_ns()
    for _ in range(CALLS):
        text_total += lib.fc_text(text, len(text))
    report(f'{prefix}text', started)

    bytes_total = 0
    started = time.perf_counter_ns()
    for _ in range(CALLS):
        bytes_total += lib.fc_bytes(data, len(data))
    report(f'{prefix}bytes', started)

    started = time.perf_counter_ns()
    for _ in range(CALLS):
        lib.fc_fill(buffer, 8)
    report(f'{prefix}fill', started)


def ctypes_library(path):
    lib = ctypes.CDLL(path)
    i64, u64, f64 = ctypes.c_int64, ctypes.c_uint64, ctypes.c_double
    lib.fc_add.argtypes, lib.fc_add.restype = [i64, i64], i64
    lib.fc_narrow.argtypes, lib.fc_narrow.restype = [ctypes.c_int32, ctypes.c_uint8], ctypes.c_int32
    lib.fc_scale.argtypes, lib.fc_scale.restype = [f64, ctypes.c_float], f64
    lib.fc_six.argtypes, lib.fc_six.restype = [i64] * 6, i64
    lib.fc_text.argtypes, lib.fc_text.restype = [ctypes.c_char_p, u64], u64
    lib.fc_bytes.argtypes, lib.fc_bytes.restype = [ctypes.c_char_p, u64], u64
    lib.fc_fill.argtypes, lib.fc_fill.restype = [ctypes.c_char_p, u64], None
    return lib


CFFI_DECLARATIONS = '''
int64_t fc_add(int64_t a, int64_t b);
int32_t fc_narrow(int32_t a, uint8_t b);
double fc_scale(double value, float factor);
int64_t fc_six(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e, int64_t f);
uint64_t fc_text(const uint8_t *text, uint64_t length);
uint64_t fc_bytes(const uint8_t *bytes, uint64_t length);
void fc_fill(uint8_t *bytes, uint64_t length);
'''


def main():
    path = sys.argv[1]
    text = b'hello, world'
    data = bytes([1, 2, 3, 4, 5, 6, 7, 8])
    run('ctypes_', ctypes_library(path), text, data, ctypes.create_string_buffer(8))
    try:
        import cffi
    except ImportError:
        return
    ffi = cffi.FFI()
    ffi.cdef(CFFI_DECLARATIONS)
    run('cffi_', ffi.dlopen(path), text, data, ffi.new('uint8_t[8]'))


if __name__ == '__main__':
    main()
