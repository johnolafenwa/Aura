"""The Python side of the stored closure benchmark."""
import time


class Total:
    def __init__(self):
        self.sum = 0

    def add(self, amount):
        self.sum += amount
        return self.sum


class Rule:
    def __init__(self, weight, step):
        self.weight = weight
        self.step = step


def make_adder(amount):
    return lambda value: value + amount


def apply(step, value):
    return step(value)


def running_total():
    total = Total()
    return lambda amount: total.add(amount)


def report(name, calls, start, acc):
    print(f"{name} {calls} {int((time.perf_counter() - start) * 1000)} {acc}")


def list_call(count, reps):
    steps = []
    for index in range(count):
        if index % 2 == 0:
            steps.append(make_adder(index))
        else:
            steps.append(lambda value, index=index: value ^ index)
    acc = 0
    start = time.perf_counter()
    for _ in range(reps):
        for step in steps:
            acc = step(acc) & 1048575
    report("list_call", count * reps, start, acc)


def field_call(calls):
    rule = Rule(3, make_adder(7))
    acc = 0
    start = time.perf_counter()
    for _ in range(calls):
        acc = rule.step(acc + rule.weight) & 1048575
    report("field_call", calls, start, acc)


def mutable_call(calls):
    tally = running_total()
    acc = 0
    start = time.perf_counter()
    for index in range(calls):
        acc = tally(index & 7)
    report("mutable_call", calls, start, acc)


def create_call(calls):
    acc = 0
    start = time.perf_counter()
    for index in range(calls):
        step = make_adder(index & 15)
        acc = step(acc) & 1048575
    report("create_call", calls, start, acc)


def argument_call(calls):
    acc = 0
    start = time.perf_counter()
    for index in range(calls):
        acc = apply(lambda value: value + index, acc) & 1048575
    report("argument_call", calls, start, acc)


def main():
    list_call(64, 500_000)
    field_call(30_000_000)
    mutable_call(30_000_000)
    create_call(10_000_000)
    argument_call(10_000_000)


main()
