"""Compare ctypes calls and union reads with src/v2.au and v2.rs."""
import ctypes as c
import sys
import time


class Pixel(c.Structure):
    _fields_ = [("x", c.c_int32), ("y", c.c_int32)]


class Samples(c.Structure):
    _fields_ = [("lanes", c.c_int32 * 4)]


class Word(c.Union):
    _fields_ = [("code", c.c_int32), ("bits", c.c_uint32)]


def report(name, started, total, expected, count):
    elapsed = time.perf_counter_ns() - started
    assert total == expected * count
    print(name, elapsed / count)


def main():
    library = c.CDLL(sys.argv[1])
    callback = c.CFUNCTYPE(c.c_int64, c.c_char_p)
    library.fc_pixels.argtypes = [c.POINTER(Pixel), c.c_int32]
    library.fc_pixels.restype = c.c_int64
    library.fc_samples.argtypes = [Samples]
    library.fc_samples.restype = c.c_int64
    library.fc_words.argtypes = [c.POINTER(Word), c.c_int32]
    library.fc_words.restype = None
    library.fc_text_callback.argtypes = [callback]
    library.fc_text_callback.restype = c.c_int64

    @callback
    def character_count(text):
        return len(text.decode("utf-8"))

    pixels = (Pixel * 4)(Pixel(1, 2), Pixel(3, 4), Pixel(5, 6), Pixel(7, 8))
    samples = Samples((c.c_int32 * 4)(1, 2, 3, 4))
    words = (Word * 4)()
    library.fc_words(words, 4)
    count = 2_000_000

    total = 0
    started = time.perf_counter_ns()
    for _ in range(count):
        total += library.fc_pixels(pixels, 4)
    report("struct_array", started, total, 36, count)

    total = 0
    started = time.perf_counter_ns()
    for _ in range(count):
        total += library.fc_samples(samples)
    report("array_field", started, total, 10, count)

    total = 0
    started = time.perf_counter_ns()
    for i in range(count):
        total += words[i % 4].code
    report("union_read", started, total, 4, count)

    total = 0
    started = time.perf_counter_ns()
    for _ in range(count):
        total += library.fc_text_callback(character_count)
    report("text_callback", started, total, 4, count)


if __name__ == "__main__":
    main()
