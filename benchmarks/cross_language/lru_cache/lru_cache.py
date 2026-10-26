"""Caching: a least-recently-used cache with string keys.

Runs N cache-aside lookups against an LRU cache of 2,000 entries. The cache is a
dictionary from key to slot plus a doubly linked list kept in parallel lists,
so a hit moves its slot to the front and a full cache evicts from the back.
Keys come from a skewed mix: a small hot set and a long tail. Reports hits,
misses, evictions, a checksum of the values read, and the most recent keys.
"""
import sys

CAPACITY = 2000
HEAD = 0
TAIL = 1
MOD = 1000000007


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


class LruCache:
    def __init__(self, capacity):
        self.capacity = capacity
        self.index = {}
        self.keys = ["", ""]
        self.values = [0, 0]
        self.prev = [HEAD, HEAD]
        self.next = [TAIL, TAIL]
        self.evictions = 0

    def unlink(self, slot):
        before = self.prev[slot]
        after = self.next[slot]
        self.next[before] = after
        self.prev[after] = before

    def push_front(self, slot):
        first = self.next[HEAD]
        self.prev[slot] = HEAD
        self.next[slot] = first
        self.prev[first] = slot
        self.next[HEAD] = slot

    def get(self, key):
        slot = self.index.get(key)
        if slot is None:
            return -1
        self.unlink(slot)
        self.push_front(slot)
        return self.values[slot]

    def put(self, key, value):
        slot = self.index.get(key)
        if slot is not None:
            self.values[slot] = value
            self.unlink(slot)
            self.push_front(slot)
            return
        if len(self.index) == self.capacity:
            slot = self.prev[TAIL]
            self.unlink(slot)
            del self.index[self.keys[slot]]
            self.evictions += 1
            self.keys[slot] = key
            self.values[slot] = value
        else:
            slot = len(self.keys)
            self.keys.append(key)
            self.values.append(value)
            self.prev.append(HEAD)
            self.next.append(TAIL)
        self.index[key] = slot
        self.push_front(slot)


def main():
    n = int(sys.argv[1])
    rng = Rng(53)
    cache = LruCache(CAPACITY)
    hits = 0
    misses = 0
    checksum = 0
    for _ in range(n):
        r = rng.next()
        if r % 4 == 0:
            k = (r // 4) % 20000
        else:
            k = (r // 4) % 1500
        key = "user:" + str(k)
        value = cache.get(key)
        if value < 0:
            misses += 1
            value = k * 7 + len(key)
            cache.put(key, value)
        else:
            hits += 1
        checksum = (checksum * 31 + value) % MOD
    recent = []
    slot = cache.next[HEAD]
    while slot != TAIL and len(recent) < 5:
        recent.append(cache.keys[slot])
        slot = cache.next[slot]
    print(
        f"lru_cache ops={n} hits={hits} misses={misses} evictions={cache.evictions} "
        f"size={len(cache.index)} checksum={checksum} recent={','.join(recent)}"
    )


if __name__ == "__main__":
    main()
