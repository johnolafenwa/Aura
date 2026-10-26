"""Checksums and content-defined chunking over a byte buffer.

Generates N bytes from 64 random 200-byte blocks (some with one byte changed),
then computes FNV-1a (32-bit), CRC-32 (table driven), and Adler-32 over the
whole buffer. It also splits the buffer into chunks where a rolling hash over a
16-byte window hits a boundary, as backup and sync tools do, and counts the
distinct chunks by their FNV-1a hash.
"""
import sys

WINDOW = 16
BASE = 257
MOD = 1000000007


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def generate(rng, n):
    blocks = []
    for _ in range(64):
        block = bytearray()
        for _ in range(200):
            block.append(rng.next() % 256)
        blocks.append(block)
    data = bytearray()
    while len(data) < n:
        r = rng.next()
        if r % 5 == 0:
            block = bytearray(blocks[r % 64])
            block[(r // 5) % 200] = (r // 1000) % 256
            data += block
        else:
            data += blocks[r % 64]
    return data[:n]


def fnv1a(data, start, end):
    h = 2166136261
    for i in range(start, end):
        h = ((h ^ data[i]) * 16777619) % 4294967296
    return h


def crc_table():
    table = []
    for i in range(256):
        c = i
        for _ in range(8):
            if c & 1:
                c = 0xEDB88320 ^ (c >> 1)
            else:
                c = c >> 1
        table.append(c)
    return table


def crc32(table, data):
    c = 0xFFFFFFFF
    for b in data:
        c = table[(c ^ b) & 255] ^ (c >> 8)
    return c ^ 0xFFFFFFFF


def adler32(data):
    a = 1
    b = 0
    for x in data:
        a = (a + x) % 65521
        b = (b + a) % 65521
    return b * 65536 + a


def main():
    n = int(sys.argv[1])
    rng = Rng(43)
    data = generate(rng, n)
    whole = fnv1a(data, 0, len(data))
    crc = crc32(crc_table(), data)
    adler = adler32(data)

    power = 1
    for _ in range(WINDOW - 1):
        power = power * BASE % MOD
    seen = set()
    chunks = 0
    chunk_sum = 0
    start = 0
    h = 0
    for i in range(len(data)):
        if i >= WINDOW:
            h = (h + MOD - data[i - WINDOW] * power % MOD) % MOD
        h = (h * BASE + data[i]) % MOD
        size = i + 1 - start
        if size >= 32 and (h % 128 == 0 or size >= 1024):
            digest = fnv1a(data, start, i + 1)
            seen.add(digest)
            chunks += 1
            chunk_sum = (chunk_sum + digest % 1000003 * size) % MOD
            start = i + 1
    if start < len(data):
        digest = fnv1a(data, start, len(data))
        seen.add(digest)
        chunks += 1
        chunk_sum = (chunk_sum + digest % 1000003 * (len(data) - start)) % MOD
    print(
        f"hashing bytes={len(data)} fnv={whole} crc32={crc} adler32={adler} "
        f"chunks={chunks} unique={len(seen)} chunk_sum={chunk_sum}"
    )


if __name__ == "__main__":
    main()
