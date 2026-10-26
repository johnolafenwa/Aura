"""JSON parse and serialize: an API-style payload parsed, walked, and dumped N times.

Builds a 1,000-record order payload, serializes it once, then each round
parses the text, walks the tree for statistics, and serializes it again.
"""
import json
import sys

RECORDS = 1000


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def build_payload(rng):
    orders = []
    for i in range(RECORDS):
        r = rng.next()
        note = None
        if r % 4 == 0:
            note = f'gift "wrap" \\ {i}\n'
        orders.append({
            "id": i,
            "customer": f"customer-{r % 500}",
            "price": (2 * (r % 50000) + 1) / 1000,
            "quantity": 1 + (r // 7) % 20,
            "express": r % 3 == 0,
            "note": note,
            "items": [{"sku": f"SKU-{(r // 11) % 900}", "units": 1 + (r // 13) % 5} for _ in range(1 + r % 3)],
            "address": {"city": f"city-{r % 40}", "zip": 10000 + r % 90000},
        })
    return {"version": 3, "source": "benchmark", "orders": orders}


class Stats:
    def __init__(self):
        self.ints = 0
        self.floats = 0
        self.strings = 0
        self.string_bytes = 0
        self.nulls = 0
        self.bools = 0
        self.containers = 0


def walk(value, stats):
    if value is None:
        stats.nulls += 1
    elif value is True or value is False:
        stats.bools += 1
    elif isinstance(value, int):
        stats.ints += value
    elif isinstance(value, float):
        stats.floats += round(value * 1000)
    elif isinstance(value, str):
        stats.strings += 1
        stats.string_bytes += len(value.encode())
    elif isinstance(value, list):
        stats.containers += 1
        for item in value:
            walk(item, stats)
    else:
        stats.containers += 1
        for item in value.values():
            walk(item, stats)


def main():
    rounds = int(sys.argv[1])
    text = json.dumps(build_payload(Rng(5)), sort_keys=True, separators=(",", ":"))
    stats = Stats()
    dumped_bytes = 0
    for _ in range(rounds):
        value = json.loads(text)
        walk(value, stats)
        again = json.dumps(value, sort_keys=True, separators=(",", ":"))
        if again != text:
            raise RuntimeError("round trip changed the text")
        dumped_bytes += len(again)
    print(
        f"json_roundtrip rounds={rounds} text_bytes={len(text)} ints={stats.ints} floats={stats.floats} "
        f"strings={stats.strings} string_bytes={stats.string_bytes} nulls={stats.nulls} "
        f"bools={stats.bools} containers={stats.containers} dumped_bytes={dumped_bytes}"
    )


if __name__ == "__main__":
    main()
