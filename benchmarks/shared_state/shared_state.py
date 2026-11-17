"""Shared state benchmark, the Python side. See shared_state.au."""

import threading
import time

LOCK_STEPS = 4_000_000
ATOMIC_STEPS = 20_000_000
DICT_STEPS = 4_000_000
CACHE_STEPS = 1_000_000
CACHE_KEYS = 1024
TASKS = 4


class Counter:
    """A counter behind a lock, which is how Python shares an integer."""

    def __init__(self):
        self.lock = threading.Lock()
        self.value = 0


def lock_steps(counter, steps):
    lock = counter.lock
    for _ in range(steps):
        with lock:
            counter.value += 1


def dict_steps(lock, table, steps):
    for i in range(steps):
        with lock:
            table[i % CACHE_KEYS] = i


def cache_steps(lock, cache, seed, steps, totals, slot):
    state = seed
    total = 0
    for _ in range(steps):
        state = (state * 48271) % 2147483647
        key = state % CACHE_KEYS
        with lock:
            known = cache.get(key)
        if known is None:
            with lock:
                cache[key] = key * 3
            total += key * 3
        else:
            total += known
    totals[slot] = total


def run_tasks(target, argument_lists):
    workers = [threading.Thread(target=target, args=args) for args in argument_lists]
    for worker in workers:
        worker.start()
    for worker in workers:
        worker.join()


def milliseconds(start):
    return int((time.perf_counter() - start) * 1000)


def main():
    counter = Counter()
    start = time.perf_counter()
    lock_steps(counter, LOCK_STEPS)
    print(f"lock {milliseconds(start)} {counter.value}")

    # Python has no atomic integer: a counter several threads change needs
    # the same lock.
    counter = Counter()
    start = time.perf_counter()
    lock_steps(counter, ATOMIC_STEPS)
    print(f"atomic {milliseconds(start)} {counter.value}")

    lock = threading.Lock()
    table = {}
    start = time.perf_counter()
    dict_steps(lock, table, DICT_STEPS)
    print(f"dict {milliseconds(start)} {len(table)} {sum(table.values())}")

    counter = Counter()
    start = time.perf_counter()
    run_tasks(lock_steps, [(counter, LOCK_STEPS // TASKS)] * TASKS)
    print(f"lock_shared {milliseconds(start)} {counter.value}")

    counter = Counter()
    start = time.perf_counter()
    run_tasks(lock_steps, [(counter, ATOMIC_STEPS // TASKS)] * TASKS)
    print(f"atomic_shared {milliseconds(start)} {counter.value}")

    lock = threading.Lock()
    cache = {}
    totals = [0] * TASKS
    start = time.perf_counter()
    run_tasks(
        cache_steps,
        [(lock, cache, seed, CACHE_STEPS, totals, slot) for slot, seed in enumerate([11, 23, 37, 41])],
    )
    print(f"cache_shared {milliseconds(start)} {sum(totals)} {len(cache)}")


if __name__ == "__main__":
    main()
