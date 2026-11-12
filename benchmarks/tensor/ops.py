"""The tensor benchmark in NumPy, PyTorch, and MLX.

    python3 ops.py numpy
    python3 ops.py torch-cpu
    python3 ops.py torch-mps
    python3 ops.py mlx

Each row prints `<name> <size> <milliseconds> <checksum>`. A side whose
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

            lib = np
            exp, log, sqrt = np.exp, np.log, np.sqrt
            absolute, maximum, matmul = np.abs, np.maximum, np.matmul
        elif side_name.startswith("torch"):
            import torch

            device = torch.device("mps" if side_name == "torch-mps" else "cpu")
            if side_name == "torch-mps" and not torch.backends.mps.is_available():
                print("torch has no mps device here")
                return

            def make(values):
                return torch.tensor(values, dtype=torch.float32, device=device)

            def done(value):
                if device.type == "mps":
                    torch.mps.synchronize()
                return value

            def item(value):
                return float(value.item())

            exp, log, sqrt = torch.exp, torch.log, torch.sqrt
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

            exp, log, sqrt = mx.exp, mx.log, mx.sqrt
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
        start = time.perf_counter()
        for _ in range(reps):
            done(x + y)
        print(f"add {side * side} {ms(start)} {item((x + y).sum())}")

        start = time.perf_counter()
        for _ in range(reps):
            done(sqrt(x * y + x))
        print(f"mul_add_sqrt {side * side} {ms(start)} {item(sqrt(x * y + x).sum())}")

        acc = 0.0
        start = time.perf_counter()
        for _ in range(reps):
            acc += item((x * y).sum())
        print(f"mul_sum {side * side} {ms(start)} {acc}")

        start = time.perf_counter()
        for _ in range(reps):
            done(maximum(absolute(log(exp(x)) - x), y))
        print(f"exp_log_max {side * side} {ms(start)} {item(maximum(absolute(log(exp(x)) - x), y).max())}")

    def products(side, reps):
        a = grid(side, 0.001)
        b = grid(side, 0.002)
        acc = 0.0
        start = time.perf_counter()
        for _ in range(reps):
            acc += item(matmul(a, b).max())
        print(f"matmul {side}x{side} {ms(start)} {acc}")

    print(f"side {side_name}")
    elementwise(32, 2000)
    elementwise(4096, 20)
    products(64, 500)
    products(1024, 10)


if __name__ == "__main__":
    main()
