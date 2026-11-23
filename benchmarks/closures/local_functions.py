import sys
import time


def measure_local(seed, count):
    def step(value):
        return (value * 3 + seed) & 1048575

    result = 1
    start = time.perf_counter()
    for _ in range(count):
        result = step(result)
    print(f"local {count} {int((time.perf_counter() - start) * 1000)} {result}")


if __name__ == "__main__":
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 7
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 48_000_000
    measure_local(seed, count)
