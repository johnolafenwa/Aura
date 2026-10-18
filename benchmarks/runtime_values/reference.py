"""Python reference timings for the runtime value benchmarks."""
import sys
import time


def dict_build_get():
    n = 20000
    index = {}
    for i in range(n):
        index[f"key-{i}"] = i
    total = 0
    for _ in range(10):
        for j in range(n):
            value = index.get(f"key-{j}")
            total += value if value is not None else -1
    return total


def set_insert_contains():
    n = 20000
    seen = set()
    for i in range(n):
        seen.add(i * 7)
    return sum(1 for j in range(n * 10) if j in seen)


def string_list_scan():
    names = [f"user-{i % 100}" for i in range(10000)]
    hits = 0
    for _ in range(100):
        for name in names:
            if name == "user-7":
                hits += 1
    return hits


def int_list_sum():
    values = list(range(100000))
    total = 0
    for _ in range(20):
        for value in values:
            total += value
    return total


def record_field_reads():
    docs = []
    for i in range(5000):
        docs.append({"name": f"user-{i}", "age": 18 + i % 60, "city": "Lagos", "active": i % 3 != 0})
    adults = 0
    for _ in range(20):
        for doc in docs:
            age = doc.get("age")
            if isinstance(age, int) and age >= 40:
                adults += 1
    return adults


PROGRAMS = {
    "dict_build_get": dict_build_get,
    "set_insert_contains": set_insert_contains,
    "string_list_scan": string_list_scan,
    "int_list_sum": int_list_sum,
    "record_field_reads": record_field_reads,
}


if __name__ == "__main__":
    names = sys.argv[1:] or list(PROGRAMS)
    for name in names:
        started = time.perf_counter()
        result = PROGRAMS[name]()
        print(f"{name} {result} {time.perf_counter() - started:.3f}")
