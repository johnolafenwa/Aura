"""The NumPy side of the Array view benchmark."""

import time

import numpy as np


def matrix(n):
    return np.array([(i % 97) * 0.25 + 1.0 for i in range(n * n)]).reshape(n, n)


def report(name, size, start, acc):
    print(f"{name} {size} {int((time.perf_counter() - start) * 1000)} {acc}")


def row_sums(n, reps):
    m = matrix(n)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        for i in range(n):
            acc += m[i, :].sum()
    report("row_sums", n, start, acc)


def column_sums(n, reps):
    m = matrix(n)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        for j in range(n):
            acc += m[:, j].sum()
    report("column_sums", n, start, acc)


def transposed_add(n, reps):
    m = matrix(n)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        t = m.transpose()
        acc += (t + m)[n - 1, 0]
    report("transposed_add", n, start, acc)


def stencil(n, reps):
    u = np.array([float(i % 1000) for i in range(n)])
    start = time.perf_counter()
    for _ in range(reps):
        u[1 : n - 1] = (u[0 : n - 2] + u[2:n]) * 0.5
    report("stencil", n, start, u[n // 2])


def row_swaps(n, reps):
    m = matrix(n)
    start = time.perf_counter()
    for _ in range(reps):
        for i in range(n // 2):
            upper = m[i, :].copy()
            m[i, :] = m[n - 1 - i, :]
            m[n - 1 - i, :] = upper
    report("row_swaps", n, start, m[0, 1] + m[n - 1, 2])


def column_scale(n, reps):
    m = matrix(n)
    start = time.perf_counter()
    for _ in range(reps):
        for j in range(n):
            m[:, j] *= 1.0001
    report("column_scale", n, start, m[n - 1, n - 1])


def reshape_reverse(n, reps):
    m = matrix(n)
    acc = 0.0
    start = time.perf_counter()
    for rep in range(reps):
        flat = m.reshape(n * n)
        acc += flat[::-1][rep % n] + flat[::2].sum()
    report("reshape_reverse", n, start, acc)


row_sums(512, 200)
column_sums(512, 200)
transposed_add(512, 200)
stencil(1000000, 200)
row_swaps(512, 400)
column_scale(512, 100)
reshape_reverse(512, 400)
