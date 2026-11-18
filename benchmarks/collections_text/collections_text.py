import math
import time


def report(name, start, checksum):
    print(name, (time.perf_counter_ns() - start) / 1_000_000, checksum)


def main():
    checksum = 0
    start = time.perf_counter_ns()
    for i in range(500_000):
        checksum += math.gcd(i % 997 + 41, 1533)
    report("gcd", start, checksum)

    text = "abracadabra"
    checksum = 0
    start = time.perf_counter_ns()
    for i in range(400_000):
        if text[i % 11] == "a":
            checksum += 1
    report("ascii_index", start, checksum)

    left = set(range(128))
    right = set(range(64, 192))
    checksum = 0
    start = time.perf_counter_ns()
    for _ in range(2000):
        checksum += len(left | right)
    report("set_union", start, checksum)

    message = "café-😀|" * 32
    checksum = 0
    start = time.perf_counter_ns()
    for _ in range(4000):
        checksum += len(message.encode("utf-16-le").decode("utf-16-le"))
    report("utf16_round_trip", start, checksum)


if __name__ == "__main__":
    main()
