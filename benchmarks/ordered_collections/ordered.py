from bisect import bisect_left, insort_left
from collections import deque
from heapq import heappop, heappush
from time import perf_counter

COUNT = 8_192

def row(label, start, value):
    print(f"{label} {(perf_counter() - start) * 1000:.3f} {value}")

def run():
    start = perf_counter()
    queue = deque()
    for i in range(COUNT):
        if i % 2 == 0:
            queue.appendleft(i)
        else:
            queue.append(i)
    result = 0
    for i in range(COUNT):
        if i % 2 == 0:
            result += queue.popleft()
        else:
            result += queue.pop()
    row("deque", start, result)

    start = perf_counter()
    heap = []
    for i in range(COUNT):
        heappush(heap, (i * 40503) % COUNT)
    result = 0
    for i in range(COUNT):
        result += heappop(heap) * i
    row("heap", start, result)

    start = perf_counter()
    keys, values = [], {}
    for i in range(COUNT):
        key = (i * 40503) % COUNT
        if key not in values:
            insort_left(keys, key)
        values[key] = i
    result = 0
    for i in range(COUNT):
        result += values[i]
    row("map", start, result)

    start = perf_counter()
    keys = []
    for i in range(COUNT):
        key = (i * 40503) % COUNT
        at = bisect_left(keys, key)
        if at == len(keys) or keys[at] != key:
            keys.insert(at, key)
    result = 0
    for i in range(COUNT):
        at = bisect_left(keys, i)
        if at < len(keys) and keys[at] == i:
            keys.pop(at)
            result += i
    row("set", start, result)

if __name__ == "__main__":
    run()
