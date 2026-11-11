"""Streaming I/O: a log read in line batches and in chunks, with gzip and HMAC.

Writes N log lines, reads them back line by line and parses each, then reads
the file again in 64 KiB chunks. Each chunk is compressed with gzip,
decompressed again, and folded into a running HMAC-SHA256, so the program never
holds the whole file. Takes the line count and a scratch directory.
"""
import gzip
import hashlib
import hmac
import os
import sys


class Rng:
    def __init__(self, state):
        self.state = state

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def main():
    n = int(sys.argv[1])
    workdir = sys.argv[2]
    rng = Rng(41)
    words = ["connect", "read", "write", "close", "retry", "timeout", "accept", "flush"]

    log_path = os.path.join(workdir, "stream.log")
    with open(log_path, "w", encoding="utf-8", newline="\n") as log:
        for i in range(n):
            word = words[rng.next() % 8]
            log.write(f"{i} {word} worker-{rng.next() % 64} {rng.next() % 100000}\n")

    lines = 0
    value_sum = 0
    retries = 0
    with open(log_path, encoding="utf-8", newline="\n") as file:
        for line in file:
            fields = line.rstrip("\n").split(" ")
            lines += 1
            value_sum += int(fields[3])
            if fields[1] == "retry":
                retries += 1

    chunks = 0
    restored = 0
    smaller = 0
    mac = b"stream_io"
    with open(log_path, "rb") as file:
        while True:
            chunk = file.read(65536)
            if not chunk:
                break
            chunks += 1
            packed = gzip.compress(chunk, 6)
            if len(packed) < len(chunk):
                smaller += 1
            plain = gzip.decompress(packed)
            restored += len(plain)
            mac = hmac.new(mac, plain, hashlib.sha256).digest()

    os.remove(log_path)
    print(
        f"stream_io lines={lines} value_sum={value_sum} retries={retries} chunks={chunks} "
        f"smaller={smaller} restored={restored} mac={mac.hex()}"
    )


if __name__ == "__main__":
    main()
