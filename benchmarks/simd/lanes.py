"""The Python side of the lane value benchmark: the same kernels as plain
loops over lists, at one hundredth of the repetitions. Python has no lane
values, so each loop handles one element at a time and the sums add in
source order."""

import math
import time


def dot(xs, ys):
    total = 0.0
    for i in range(len(xs)):
        total += xs[i] * ys[i]
    return total


def saxpy(a, xs, ys):
    for i in range(len(xs)):
        ys[i] = xs[i] * a + ys[i]


def norms(xs, ys, out):
    for i in range(len(xs)):
        out[i] = math.sqrt(xs[i] * xs[i] + ys[i] * ys[i])


def clamp(xs, low, high):
    for i in range(len(xs)):
        xs[i] = min(max(xs[i], low), high)


def absdiff(xs, ys, out):
    for i in range(len(xs)):
        out[i] = abs(xs[i] - ys[i])


def total_of(xs):
    total = 0.0
    for x in xs:
        total += x
    return total


def timed(name, n, work):
    start = time.perf_counter()
    result = work()
    print(f"{name} {n} {round((time.perf_counter() - start) * 1000)} {result}")


def main():
    n = 1024
    reps = 4000
    xs = [(i % 1000) * 0.5 + 1.0 for i in range(n)]
    ys = [(i % 1000) * 0.25 - 3.0 for i in range(n)]
    out = [0.0] * n
    fx = [(i % 100) * 0.5 for i in range(n)]
    fy = [(i % 70) * 0.25 for i in range(n)]
    fo = [0.0] * n

    def repeat(work):
        result = None
        for _ in range(reps):
            result = work()
        return result

    timed("dot", n, lambda: repeat(lambda: dot(xs, ys)))
    timed("saxpy", n, lambda: repeat(lambda: saxpy(0.0000001, xs, ys)) or ys[7])
    timed("norms", n, lambda: repeat(lambda: norms(xs, ys, out)) or out[7])
    timed("clamp", n, lambda: repeat(lambda: clamp(out, 3.0, 200.0)) or out[7])
    timed("absdiff", n, lambda: repeat(lambda: absdiff(fx, fy, fo)) or fo[7])
    timed("sum8", n, lambda: repeat(lambda: total_of(fx)))


if __name__ == "__main__":
    main()
