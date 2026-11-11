"""The Python side of the list loop benchmark: the same indexed loops over
lists, at one hundredth of the repetitions."""

import time


def scale(xs, factor):
    for i in range(len(xs)):
        xs[i] *= factor


def saxpy(a, xs, ys):
    for i in range(len(xs)):
        ys[i] = a * xs[i] + ys[i]


def blur(xs, out):
    for i in range(1, len(xs) - 1):
        out[i] = (xs[i - 1] + xs[i] + xs[i + 1]) / 3.0


def clamp(xs, low, high):
    for i in range(len(xs)):
        x = xs[i]
        if x < low:
            xs[i] = low
        elif x > high:
            xs[i] = high


def dot(xs, ys):
    total = 0.0
    for i in range(len(xs)):
        total += xs[i] * ys[i]
    return total


def run(n, reps):
    xs = [(i % 1000) * 0.5 + 1.0 for i in range(n)]
    ys = [(i % 1000) * 0.25 - 3.0 for i in range(n)]
    out = [0.0] * n

    def timed(name, body, result):
        start = time.perf_counter()
        for _ in range(reps):
            body()
        print(f"{name} {n} {int((time.perf_counter() - start) * 1000)} {result()}")

    timed("scale", lambda: scale(ys, 1.0000001), lambda: sum(ys))
    timed("saxpy", lambda: saxpy(0.0000001, xs, ys), lambda: sum(ys))
    timed("blur", lambda: blur(ys, out), lambda: sum(out))
    timed("clamp", lambda: clamp(out, -1.0, 200.0), lambda: sum(out))
    acc = [0.0]

    def add_dot():
        acc[0] += dot(xs, ys)

    timed("dot", add_dot, lambda: acc[0])


run(1024, 4000)
run(1000000, 3)
