"""Compression: LZW with 12-bit codes and run-length coding, both round-tripped.

Generates N bytes of word-like text, compresses it with LZW (dictionary reset
at 4,096 entries), decompresses it, and checks the result. It also run-length
encodes a sparse byte stream and decodes it again.
"""
import sys

SYLLABLES = ["the", "and", "data", "core", "sys", "net", "run", "io", "map", "set"]
MAX_CODES = 4096


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def generate(rng, n):
    parts = []
    size = 0
    while size < n:
        r = rng.next()
        word = SYLLABLES[r % 10] + SYLLABLES[(r // 10) % 10]
        if r % 9 == 0:
            word += "\n"
        else:
            word += " "
        parts.append(word)
        size += len(word)
    return "".join(parts).encode()[:n]


def compress(data):
    table = {}
    next_code = 256
    codes = []
    prefix = data[0]
    for i in range(1, len(data)):
        byte = data[i]
        key = prefix * 256 + byte
        code = table.get(key)
        if code is not None:
            prefix = code
        else:
            codes.append(prefix)
            if next_code < MAX_CODES:
                table[key] = next_code
                next_code += 1
            else:
                table = {}
                next_code = 256
            prefix = byte
    codes.append(prefix)
    return codes


def decompress(codes):
    prefixes = []
    suffixes = []
    out = bytearray()
    previous = -1
    for code in codes:
        if code < 256:
            entry = bytes([code])
        elif code - 256 < len(prefixes):
            entry = expand(prefixes, suffixes, code)
        else:
            entry = expand(prefixes, suffixes, previous)
            entry += entry[:1]
        out += entry
        if previous >= 0:
            if len(prefixes) + 256 < MAX_CODES:
                prefixes.append(previous)
                suffixes.append(entry[0])
            else:
                prefixes = []
                suffixes = []
        previous = code
    return bytes(out)


def expand(prefixes, suffixes, code):
    reversed_bytes = bytearray()
    while code >= 256:
        reversed_bytes.append(suffixes[code - 256])
        code = prefixes[code - 256]
    reversed_bytes.append(code)
    reversed_bytes.reverse()
    return bytes(reversed_bytes)


def sparse_stream(rng, n):
    data = bytearray()
    while len(data) < n:
        r = rng.next()
        run = 1 + r % 300
        value = 0 if r % 3 != 0 else (r // 3) % 256
        data += bytes([value]) * min(run, n - len(data))
    return bytes(data)


def rle_encode(data):
    out = []
    i = 0
    while i < len(data):
        value = data[i]
        run = 1
        while i + run < len(data) and run < 255 and data[i + run] == value:
            run += 1
        out.append(run)
        out.append(value)
        i += run
    return out


def rle_decode(pairs):
    out = bytearray()
    for i in range(0, len(pairs), 2):
        out += bytes([pairs[i + 1]]) * pairs[i]
    return bytes(out)


def main():
    n = int(sys.argv[1])
    rng = Rng(29)
    data = generate(rng, n)
    codes = compress(data)
    restored = decompress(codes)
    checksum = 0
    for i, code in enumerate(codes):
        checksum = (checksum * 31 + code * (i % 7 + 1)) % 1000000007
    sparse = sparse_stream(rng, n)
    pairs = rle_encode(sparse)
    rle_ok = rle_decode(pairs) == sparse
    print(
        f"lzw_compress bytes={len(data)} codes={len(codes)} restored={str(restored == data).lower()} "
        f"checksum={checksum} rle_pairs={len(pairs) // 2} rle_restored={str(rle_ok).lower()}"
    )


if __name__ == "__main__":
    main()
