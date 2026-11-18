from decimal import Decimal
from math import factorial
from time import perf_counter


def factorials():
    return sum(factorial(n).bit_length() for n in range(4000, 4100))


def fibonacci():
    a, b = 0, 1
    for _ in range(30000):
        a, b = b, a + b
    return a.bit_length()


def small_integers():
    total = 0
    for i in range(500000):
        total += i
    return total


def modular_powers():
    return sum(pow(i, 65537, 1000000007) for i in range(1, 20001))


def money():
    price, quantity, total = Decimal("19.95"), Decimal(3), Decimal(0)
    for _ in range(200000):
        total += price * quantity
    return format(total, ".2f")


def mandelbrot():
    total = 0
    for y in range(240):
        for x in range(320):
            c = complex(x / 128.0 - 2.0, y / 120.0 - 1.0)
            z = 0j
            iteration = 0
            while iteration < 50 and z.real * z.real + z.imag * z.imag <= 4.0:
                z = z * z + c
                iteration += 1
            total += iteration
    return total


for name, run in [
    ("factorial", factorials), ("fibonacci", fibonacci), ("small_integers", small_integers),
    ("modular_powers", modular_powers), ("decimal_money", money), ("mandelbrot", mandelbrot),
]:
    start = perf_counter()
    result = run()
    print(name, f"{(perf_counter() - start) * 1000:.3f}", result)
