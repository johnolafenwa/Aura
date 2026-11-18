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
    measure_local(7, 48_000_000)
