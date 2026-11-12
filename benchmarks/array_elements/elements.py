"""The NumPy side of the Array element benchmark. The element loops are
written as the whole-array operations a NumPy program uses, since a Python
loop over elements would only measure the interpreter."""

import time
from concurrent.futures import ThreadPoolExecutor

import numpy as np


def matrix(n):
    return np.array([(i % 97) * 0.25 + 1.0 for i in range(n * n)]).reshape(n, n)


def report(name, size, start, acc):
    print(f"{name} {size} {int((time.perf_counter() - start) * 1000)} {acc}")


def element_sum(n, reps):
    m = matrix(n)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        acc += m.sum()
    report("element_sum", n, start, acc)


def element_fill(n, reps):
    m = np.zeros((n, n))
    i, j = np.indices((n, n))
    start = time.perf_counter()
    for rep in range(reps):
        m[:, :] = (i + j + rep) % 7
    report("element_fill", n, start, m.sum())


def stencil_2d(n, reps):
    grid = np.zeros((n, n))
    nxt = np.zeros((n, n))
    grid[:, 0] = 100.0
    start = time.perf_counter()
    for _ in range(reps):
        nxt[1:-1, 1:-1] = 0.25 * (
            grid[:-2, 1:-1] + grid[2:, 1:-1] + grid[1:-1, :-2] + grid[1:-1, 2:]
        )
        grid[1:-1, 1:-1] = nxt[1:-1, 1:-1]
    report("stencil_2d", n, start, grid.sum())


def center_columns(n, reps):
    m = matrix(n)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        centered = m - m.mean(axis=0)
        acc += centered[n - 1, 0]
    report("center_columns", n, start, acc)


def mask_choose(n, reps):
    m = matrix(n)
    acc = 0.0
    count = 0
    start = time.perf_counter()
    for _ in range(reps):
        large = m > 12.0
        count += int(np.count_nonzero(large))
        acc += np.where(large, m, 12.0)[n - 1, n - 1]
    report("mask_choose", n, start, acc + count)


def sqrt_exp(n, reps):
    m = matrix(n)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        acc += np.exp(np.sqrt(m) * 0.125).sum()
    report("sqrt_exp", n, start, acc)


def batched_matmul(batch, n, reps):
    stack = np.array([(i % 13) * 0.5 - 2.0 for i in range(batch * n * n)]).reshape(
        batch, n, n
    )
    weights = matrix(n)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        product = stack @ weights
        acc += product[batch - 1, n - 1, n - 1]
    report("batched_matmul", f"{batch}x{n}", start, acc)


def lane_dot(n, reps):
    xs = matrix(n).reshape(-1)
    ys = (matrix(n) * 0.5).reshape(-1)
    acc = 0.0
    start = time.perf_counter()
    for _ in range(reps):
        acc += float(xs @ ys)
    report("lane_dot", n, start, acc)


def shared_tasks(n, reps):
    m = matrix(n)
    acc = 0.0
    start = time.perf_counter()
    with ThreadPoolExecutor(4) as pool:
        for _ in range(reps):
            acc += sum(pool.map(lambda _: float(m.sum()), range(4)))
    report("shared_tasks", n, start, acc)


if __name__ == "__main__":
    element_sum(1000, 100)
    element_fill(1000, 100)
    stencil_2d(1000, 50)
    center_columns(1000, 100)
    mask_choose(1000, 100)
    sqrt_exp(1000, 20)
    batched_matmul(512, 32, 20)
    lane_dot(1000, 100)
    shared_tasks(2000, 100)
