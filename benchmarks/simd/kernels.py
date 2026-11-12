"""The Python side of the Array kernel benchmark: NumPy, and for the
smaller runs the same kernels as plain Python loops over lists."""

import sys
import time

import numpy as np


def vector(n, scale, shift):
    return [(i % 1000) * scale + shift for i in range(n)]


def report(name, size, start, acc):
    print(f"{name} {size} {int((time.perf_counter() - start) * 1000)} {acc}")


def numpy_reductions(n, reps):
    a = np.array(vector(n, 0.5, 1.0))
    b = np.array(vector(n, 0.25, -3.0))
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        acc += a.sum()
    report("sum", n, start, acc)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        acc += a.dot(b)
    report("dot", n, start, acc)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        acc += a.min() + a.max()
    report("min_max", n, start, acc)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        c = a + b
        acc += c[n - 1]
    report("add", n, start, acc)


def numpy_products(m, reps):
    p = np.array([(i % 97) * 0.01 for i in range(m * m)]).reshape(m, m)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        q = p @ p
        acc += q[m - 1, m - 1]
    report("matmul", m, start, acc)


def python_reductions(n, reps):
    a = vector(n, 0.5, 1.0)
    b = vector(n, 0.25, -3.0)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        acc += sum(a)
    report("sum", n, start, acc)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        acc += sum(x * y for x, y in zip(a, b))
    report("dot", n, start, acc)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        acc += min(a) + max(a)
    report("min_max", n, start, acc)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        c = [x + y for x, y in zip(a, b)]
        acc += c[n - 1]
    report("add", n, start, acc)


def python_products(m, reps):
    p = [[((i * m + j) % 97) * 0.01 for j in range(m)] for i in range(m)]
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        columns = list(zip(*p))
        q = [[sum(x * y for x, y in zip(row, column)) for column in columns] for row in p]
        acc += q[m - 1][m - 1]
    report("matmul", m, start, acc)


def main():
    if sys.argv[1:] == ["python"]:
        # One hundredth of the repetitions; multiply the times by 100.
        python_reductions(1024, 4000)
        python_reductions(1000000, 3)
        python_products(64, 20)
        return
    numpy_reductions(1024, 400000)
    numpy_reductions(1000000, 300)
    numpy_products(64, 2000)
    numpy_products(512, 4)


main()
