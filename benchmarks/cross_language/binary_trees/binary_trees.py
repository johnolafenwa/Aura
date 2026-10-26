"""Allocation: build and walk many complete binary trees (Benchmarks Game).

Builds a stretch tree of depth N + 1, keeps one long-lived tree of depth N, and
for each even depth from 4 to N builds 2^(N - depth + 4) short-lived trees,
counting the nodes of each. It measures allocation, recursion, and freeing.
"""
import sys


def make(depth):
    if depth == 0:
        return (None, None)
    return (make(depth - 1), make(depth - 1))


def check(node):
    left, right = node
    if left is None:
        return 1
    return 1 + check(left) + check(right)


def main():
    n = max(6, int(sys.argv[1]))
    stretch = check(make(n + 1))
    long_lived = make(n)
    rows = []
    for depth in range(4, n + 1, 2):
        iterations = 2 ** (n - depth + 4)
        total = 0
        for _ in range(iterations):
            total += check(make(depth))
        rows.append(str(depth) + ":" + str(iterations) + ":" + str(total))
    print(f"binary_trees depth={n} stretch={stretch} rows={','.join(rows)} long_lived={check(long_lived)}")


if __name__ == "__main__":
    main()
