"""The Python side of the integer lane benchmark: the same kernels as
plain loops over lists, at one hundredth of the repetitions."""

import time


def add(xs, ys, out):
    for i in range(len(xs)):
        out[i] = xs[i] + ys[i]


def total(xs):
    s = 0
    for x in xs:
        s += x
    return s


def main():
    n = 1024
    reps = 4000
    xs = [(i % 1000) * 3 + 1 for i in range(n)]
    ys = [(i % 700) * 5 - 3 for i in range(n)]
    out = [0] * n

    start = time.perf_counter()
    for _ in range(reps):
        add(xs, ys, out)
    print(f"add {n} {int((time.perf_counter() - start) * 1000)} {out[7]}")

    acc = 0
    start = time.perf_counter()
    for _ in range(reps):
        acc += total(xs)
    print(f"total {n} {int((time.perf_counter() - start) * 1000)} {acc}")


if __name__ == "__main__":
    main()
