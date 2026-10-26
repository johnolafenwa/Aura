"""Document database: schemaless JSON records with a primary and a secondary index.

Inserts N documents, runs point reads, an indexed filter query, in-place
updates, deletes, a per-city aggregate, then saves every live document as
JSON lines and loads the file back into a fresh database.
"""
import json
import os
import sys

CITIES = ["Lagos", "Abuja", "Accra", "Nairobi", "Cairo", "Dakar", "Kigali", "Tunis"]


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


class Database:
    def __init__(self):
        self.docs = {}
        self.by_city = {}

    def insert(self, doc):
        doc_id = doc["id"]
        city = doc["city"]
        self.docs[doc_id] = doc
        ids = self.by_city.get(city)
        if ids is None:
            self.by_city[city] = [doc_id]
        else:
            ids.append(doc_id)

    def get(self, doc_id):
        return self.docs.get(doc_id)

    def delete(self, doc_id):
        return self.docs.pop(doc_id, None) is not None

    def count_where(self, city, min_age):
        matched = 0
        for doc_id in self.by_city.get(city, []):
            doc = self.docs.get(doc_id)
            if doc is not None and doc["active"] and doc["age"] >= min_age:
                matched += 1
        return matched


def make_doc(i, r):
    return {
        "id": i,
        "name": f"user-{i}",
        "age": 18 + r % 60,
        "city": CITIES[(r // 60) % 8],
        "score": (r // 480) % 1000,
        "active": i % 3 != 0,
        "tags": [f"t{r % 7}", f"t{r % 5}"],
    }


def main():
    n = int(sys.argv[1])
    workdir = sys.argv[2]
    rng = Rng(42)
    db = Database()
    for i in range(n):
        db.insert(make_doc(i, rng.next()))

    hits = 0
    misses = 0
    score_total = 0
    for _ in range(n * 2):
        doc = db.get(rng.next() % (n + n // 10))
        if doc is None:
            misses += 1
        else:
            hits += 1
            score_total += doc["score"]

    matched = 0
    for city in CITIES:
        matched += db.count_where(city, 40)

    for doc_id in range(0, n, 10):
        doc = db.get(doc_id)
        doc["score"] += 7
        doc["tags"].append("updated")

    deleted = 0
    for doc_id in range(1, n, 20):
        if db.delete(doc_id):
            deleted += 1

    city_scores = {city: 0 for city in CITIES}
    city_counts = {city: 0 for city in CITIES}
    for doc in db.docs.values():
        city_scores[doc["city"]] += doc["score"]
        city_counts[doc["city"]] += 1
    average_total = 0
    for city in CITIES:
        if city_counts[city] > 0:
            average_total += city_scores[city] * 100 // city_counts[city]

    path = os.path.join(workdir, "docdb.jsonl")
    lines = [json.dumps(doc, sort_keys=True, separators=(",", ":")) for doc in db.docs.values()]
    text = "\n".join(lines) + "\n"
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)
    saved_bytes = len(text.encode("utf-8"))

    with open(path, encoding="utf-8") as handle:
        loaded = handle.read()
    reloaded = Database()
    for line in loaded.splitlines():
        reloaded.insert(json.loads(line))
    os.remove(path)

    checksum = 0
    for doc in reloaded.docs.values():
        checksum += doc["score"] + doc["age"] + len(doc["tags"])
    print(
        f"docdb docs={n} live={len(reloaded.docs)} hits={hits} misses={misses} "
        f"score={score_total} matched={matched} deleted={deleted} averages={average_total} "
        f"bytes={saved_bytes} checksum={checksum}"
    )


if __name__ == "__main__":
    main()
