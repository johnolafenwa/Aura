"""File I/O: a line log, a binary record file, and a directory of small files.

Writes N log lines one write call at a time, reads the log back and parses it,
writes N four-byte little-endian records and decodes them, then creates N / 100
small files in a subdirectory, lists and reads them, replaces a manifest
atomically (write a temporary file, rename it over the target), and deletes
everything it wrote. Takes the line count and a scratch directory.
"""
import os
import sys


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def main():
    n = int(sys.argv[1])
    workdir = sys.argv[2]
    rng = Rng(37)

    log_path = os.path.join(workdir, "events.log")
    with open(log_path, "w") as log:
        for i in range(n):
            log.write("event " + str(i) + " value " + str(rng.next() % 100000) + "\n")
    with open(log_path) as log:
        text = log.read()
    lines = 0
    value_sum = 0
    for line in text.split("\n"):
        if line == "":
            continue
        fields = line.split(" ")
        lines += 1
        value_sum += int(fields[3])

    records = bytearray()
    for _ in range(n):
        v = rng.next()
        records.append(v & 255)
        records.append((v >> 8) & 255)
        records.append((v >> 16) & 255)
        records.append((v >> 24) & 255)
    binary_path = os.path.join(workdir, "records.bin")
    with open(binary_path, "wb") as out:
        out.write(records)
    with open(binary_path, "rb") as source:
        loaded = source.read()
    checksum = 0
    for i in range(0, len(loaded), 4):
        v = loaded[i] | (loaded[i + 1] << 8) | (loaded[i + 2] << 16) | (loaded[i + 3] << 24)
        checksum = (checksum * 31 + v) % 1000000007

    shard = os.path.join(workdir, "shard")
    os.makedirs(shard, exist_ok=True)
    files = max(1, n // 100)
    for k in range(files):
        with open(os.path.join(shard, "part-" + str(k) + ".txt"), "w") as part:
            part.write(("shard " + str(k) + "\n") * (k % 10 + 1))
    names = sorted(os.listdir(shard))
    file_bytes = 0
    for name in names:
        with open(os.path.join(shard, name)) as part:
            file_bytes += len(part.read())
    manifest = os.path.join(workdir, "manifest.txt")
    temporary = manifest + ".tmp"
    with open(temporary, "w") as out:
        out.write("\n".join(names))
        out.flush()
        os.fsync(out.fileno())
    os.rename(temporary, manifest)
    with open(manifest) as source:
        manifest_bytes = len(source.read())

    for name in names:
        os.remove(os.path.join(shard, name))
    os.remove(manifest)
    os.remove(binary_path)
    os.remove(log_path)
    left = len(os.listdir(shard))
    print(
        f"file_io lines={lines} value_sum={value_sum} record_bytes={len(loaded)} checksum={checksum} "
        f"files={files} listed={len(names)} file_bytes={file_bytes} manifest_bytes={manifest_bytes} left={left}"
    )


if __name__ == "__main__":
    main()
