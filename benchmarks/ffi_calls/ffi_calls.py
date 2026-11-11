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


def run(prefix, lib, text, data, buffer, out, as_text, floats, structs):
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

    cstr_total = 0
    started = time.perf_counter_ns()
    for _ in range(CALLS):
        cstr_total += lib.fc_cstr(text)
    report(f'{prefix}cstr', started)

    name_total = 0
    started = time.perf_counter_ns()
    for _ in range(CALLS):
        name_total += len(as_text(lib.fc_name()))
    report(f'{prefix}name', started)

    status = 0
    started = time.perf_counter_ns()
    for i in range(CALLS):
        status += lib.fc_out(i, out)
    report(f'{prefix}out', started)

    floats_total = 0.0
    started = time.perf_counter_ns()
    for _ in range(CALLS):
        floats_total += lib.fc_floats(floats, 8)
    report(f'{prefix}floats', started)

    started = time.perf_counter_ns()
    for _ in range(CALLS):
        lib.fc_bump(floats, 8)
    report(f'{prefix}bump', started)

    point, step, triple, point_address = structs
    started = time.perf_counter_ns()
    for _ in range(CALLS):
        point = lib.fc_vec2_add(point, step)
    report(f'{prefix}vec2', started)

    started = time.perf_counter_ns()
    for _ in range(CALLS):
        triple = lib.fc_triple_next(triple)
    report(f'{prefix}triple', started)

    started = time.perf_counter_ns()
    for _ in range(CALLS):
        lib.fc_vec2_move(point_address, 0.5)
    report(f'{prefix}move', started)


class Vec2(ctypes.Structure):
    _fields_ = [('x', ctypes.c_double), ('y', ctypes.c_double)]


class Triple(ctypes.Structure):
    _fields_ = [('a', ctypes.c_int64), ('b', ctypes.c_int64), ('c', ctypes.c_int64)]


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
    lib.fc_cstr.argtypes, lib.fc_cstr.restype = [ctypes.c_char_p], u64
    lib.fc_name.argtypes, lib.fc_name.restype = [], ctypes.c_char_p
    lib.fc_out.argtypes, lib.fc_out.restype = [i64, ctypes.POINTER(i64)], ctypes.c_int32
    lib.fc_floats.argtypes, lib.fc_floats.restype = [ctypes.POINTER(f64), i64], f64
    lib.fc_bump.argtypes, lib.fc_bump.restype = [ctypes.POINTER(f64), i64], None
    lib.fc_vec2_add.argtypes, lib.fc_vec2_add.restype = [Vec2, Vec2], Vec2
    lib.fc_triple_next.argtypes, lib.fc_triple_next.restype = [Triple], Triple
    lib.fc_vec2_move.argtypes, lib.fc_vec2_move.restype = [ctypes.POINTER(Vec2), f64], None
    return lib


CFFI_DECLARATIONS = '''
int64_t fc_add(int64_t a, int64_t b);
int32_t fc_narrow(int32_t a, uint8_t b);
double fc_scale(double value, float factor);
int64_t fc_six(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e, int64_t f);
uint64_t fc_text(const uint8_t *text, uint64_t length);
uint64_t fc_bytes(const uint8_t *bytes, uint64_t length);
void fc_fill(uint8_t *bytes, uint64_t length);
uint64_t fc_cstr(const char *text);
const char *fc_name(void);
int32_t fc_out(int64_t value, int64_t *out);
double fc_floats(const double *values, int64_t count);
void fc_bump(double *values, int64_t count);
typedef struct { double x, y; } Vec2;
typedef struct { int64_t a, b, c; } Triple;
Vec2 fc_vec2_add(Vec2 a, Vec2 b);
Triple fc_triple_next(Triple t);
void fc_vec2_move(Vec2 *v, double by);
'''


def main():
    path = sys.argv[1]
    text = b'hello, world'
    data = bytes([1, 2, 3, 4, 5, 6, 7, 8])
    run(
        'ctypes_', ctypes_library(path), text, data, ctypes.create_string_buffer(8),
        ctypes.byref(ctypes.c_int64(0)), lambda name: name.decode(),
        (ctypes.c_double * 8)(1, 2, 3, 4, 5, 6, 7, 8),
        (Vec2(0.0, 0.0), Vec2(0.5, -0.25), Triple(1, 2, 3), ctypes.byref(Vec2(0.0, 0.0))),
    )
    try:
        import cffi
    except ImportError:
        return
    ffi = cffi.FFI()
    ffi.cdef(CFFI_DECLARATIONS)
    run(
        'cffi_', ffi.dlopen(path), text, data, ffi.new('uint8_t[8]'),
        ffi.new('int64_t *'), lambda name: ffi.string(name).decode(),
        ffi.new('double[8]', [1, 2, 3, 4, 5, 6, 7, 8]),
        (
            ffi.new('Vec2 *', [0.0, 0.0])[0], ffi.new('Vec2 *', [0.5, -0.25])[0],
            ffi.new('Triple *', [1, 2, 3])[0], ffi.new('Vec2 *', [0.0, 0.0]),
        ),
    )


if __name__ == '__main__':
    main()
