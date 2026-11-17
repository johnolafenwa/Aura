from time import perf_counter_ns


def checked(value):
    assert value >= 0, "negative input"
    return value


def main():
    passes, failures = 1_000_000, 100_000
    total = 0
    start = perf_counter_ns()
    for i in range(passes):
        try:
            total += checked(i)
        except AssertionError:
            raise RuntimeError("expected success")
    elapsed = (perf_counter_ns() - start) / 1_000_000
    assert total == passes * (passes - 1) // 2
    print(f"success {passes} {elapsed:.3f} {total}")

    trapped = 0
    start = perf_counter_ns()
    for i in range(failures):
        try:
            checked(-i - 1)
        except AssertionError:
            trapped += 1
    elapsed = (perf_counter_ns() - start) / 1_000_000
    assert trapped == failures
    print(f"trap {failures} {elapsed:.3f} {trapped}")


if __name__ == "__main__":
    main()
