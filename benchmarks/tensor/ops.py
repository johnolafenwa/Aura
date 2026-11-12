"""The tensor benchmark in NumPy, PyTorch, and MLX.

    python3 ops.py numpy
    python3 ops.py torch-cpu
    python3 ops.py torch-mps
    python3 ops.py torch-cuda
    python3 ops.py mlx

Each row prints `<name> <size> <milliseconds> <checksum> <first pass
milliseconds>`. A side whose
library is not installed prints one line saying so.
"""

import sys
import time


def ramp(n, scale):
    return [float((i % 1000) * scale + 1.0) for i in range(n)]


def main():
    side_name = sys.argv[1] if len(sys.argv) > 1 else "numpy"
    try:
        if side_name == "numpy":
            import numpy as np

            def make(values):
                return np.array(values, dtype=np.float32)

            def done(value):
                return value

            def item(value):
                return float(value)

            sin, cos, log, sqrt = np.sin, np.cos, np.log, np.sqrt
            absolute, maximum, matmul = np.abs, np.maximum, np.matmul
        elif side_name.startswith("torch"):
            import torch

            kind = side_name.split("-")[1]
            if kind == "mps" and not torch.backends.mps.is_available():
                print("torch has no mps device here")
                return
            if kind == "cuda" and not torch.cuda.is_available():
                print("torch has no cuda device here")
                return
            device = torch.device(kind)

            def make(values):
                return torch.tensor(values, dtype=torch.float32, device=device)

            def done(value):
                if device.type == "mps":
                    torch.mps.synchronize()
                if device.type == "cuda":
                    torch.cuda.synchronize()
                return value

            def item(value):
                return float(value.item())

            sin, cos, log, sqrt = torch.sin, torch.cos, torch.log, torch.sqrt
            absolute, maximum, matmul = torch.abs, torch.maximum, torch.matmul
        elif side_name == "mlx":
            import mlx.core as mx

            def make(values):
                return mx.array(values, dtype=mx.float32)

            def done(value):
                mx.eval(value)
                return value

            def item(value):
                return float(value.item())

            sin, cos, log, sqrt = mx.sin, mx.cos, mx.log, mx.sqrt
            absolute, maximum, matmul = mx.abs, mx.maximum, mx.matmul
        else:
            print(f"unknown side {side_name}")
            return
    except ImportError as error:
        print(f"{side_name} is not installed: {error}")
        return

    def grid(side, scale):
        row = make(ramp(side, scale))
        column = make(ramp(side, 0.5)).reshape(side, 1)
        return done(row * column)

    def ms(start):
        return int((time.perf_counter() - start) * 1000)

    def elementwise(side, reps):
        x = grid(side, 0.25)
        y = grid(side, 0.125)
        # Each row runs once before its timed passes, and the time of
        # that first pass is the last column.
        start = time.perf_counter()
        done(x + y)
        first = ms(start)
        start = time.perf_counter()
        for _ in range(reps):
            done(x + y)
        print(f"add {side * side} {ms(start)} {item((x + y).sum())} {first}")

        start = time.perf_counter()
        done(sqrt(x * y + x))
        first = ms(start)
        start = time.perf_counter()
        for _ in range(reps):
            done(sqrt(x * y + x))
        print(f"mul_add_sqrt {side * side} {ms(start)} {item(sqrt(x * y + x).sum())} {first}")

        start = time.perf_counter()
        item((x * y).sum())
        first = ms(start)
        acc = 0.0
        start = time.perf_counter()
        for _ in range(reps):
            acc += item((x * y).sum())
        print(f"mul_sum {side * side} {ms(start)} {acc} {first}")

        start = time.perf_counter()
        done(maximum(absolute(sin(x) * cos(y)), log(y)))
        first = ms(start)
        start = time.perf_counter()
        for _ in range(reps):
            done(maximum(absolute(sin(x) * cos(y)), log(y)))
        print(f"sin_cos_log_max {side * side} {ms(start)} {item(maximum(absolute(sin(x) * cos(y)), log(y)).max())} {first}")

    def products(side, reps):
        a = grid(side, 0.001)
        b = grid(side, 0.002)
        start = time.perf_counter()
        item(matmul(a, b).max())
        first = ms(start)
        acc = 0.0
        start = time.perf_counter()
        for _ in range(reps):
            acc += item(matmul(a, b).max())
        print(f"matmul {side}x{side} {ms(start)} {acc} {first}")

    print(f"side {side_name}")
    elementwise(32, 2000)
    elementwise(4096, 20)
    products(64, 500)
    products(1024, 10)


if __name__ == "__main__":
    main()
