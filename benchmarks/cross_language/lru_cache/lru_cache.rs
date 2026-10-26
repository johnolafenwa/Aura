//! Caching: a least-recently-used cache with string keys.
//!
//! Runs N cache-aside lookups against an LRU cache of 2,000 entries. The cache is a
//! dictionary from key to slot plus a doubly linked list kept in parallel lists,
//! so a hit moves its slot to the front and a full cache evicts from the back.
//! Keys come from a skewed mix: a small hot set and a long tail. Reports hits,
//! misses, evictions, a checksum of the values read, and the most recent keys.
use std::collections::HashMap;

const CAPACITY: usize = 2000;
const HEAD: usize = 0;
const TAIL: usize = 1;
const MOD: i64 = 1000000007;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

struct LruCache {
    capacity: usize,
    index: HashMap<String, usize>,
    keys: Vec<String>,
    values: Vec<i64>,
    prev: Vec<usize>,
    next: Vec<usize>,
    evictions: i64,
}

impl LruCache {
    fn new(capacity: usize) -> Self {
        LruCache {
            capacity,
            index: HashMap::new(),
            keys: vec![String::new(), String::new()],
            values: vec![0, 0],
            prev: vec![HEAD, HEAD],
            next: vec![TAIL, TAIL],
            evictions: 0,
        }
    }

    fn unlink(&mut self, slot: usize) {
        let before = self.prev[slot];
        let after = self.next[slot];
        self.next[before] = after;
        self.prev[after] = before;
    }

    fn push_front(&mut self, slot: usize) {
        let first = self.next[HEAD];
        self.prev[slot] = HEAD;
        self.next[slot] = first;
        self.prev[first] = slot;
        self.next[HEAD] = slot;
    }

    fn get(&mut self, key: &str) -> i64 {
        match self.index.get(key) {
            None => -1,
            Some(&slot) => {
                self.unlink(slot);
                self.push_front(slot);
                self.values[slot]
            }
        }
    }

    fn put(&mut self, key: String, value: i64) {
        if let Some(&slot) = self.index.get(&key) {
            self.values[slot] = value;
            self.unlink(slot);
            self.push_front(slot);
            return;
        }
        let slot;
        if self.index.len() == self.capacity {
            slot = self.prev[TAIL];
            self.unlink(slot);
            self.index.remove(&self.keys[slot]);
            self.evictions += 1;
            self.keys[slot] = key.clone();
            self.values[slot] = value;
        } else {
            slot = self.keys.len();
            self.keys.push(key.clone());
            self.values.push(value);
            self.prev.push(HEAD);
            self.next.push(TAIL);
        }
        self.index.insert(key, slot);
        self.push_front(slot);
    }
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut rng = Rng { state: 53 };
    let mut cache = LruCache::new(CAPACITY);
    let (mut hits, mut misses, mut checksum) = (0, 0, 0i64);
    for _ in 0..n {
        let r = rng.next();
        let k = if r % 4 == 0 {
            (r / 4) % 20000
        } else {
            (r / 4) % 1500
        };
        let key = format!("user:{k}");
        let mut value = cache.get(&key);
        if value < 0 {
            misses += 1;
            value = k * 7 + key.len() as i64;
            cache.put(key, value);
        } else {
            hits += 1;
        }
        checksum = (checksum * 31 + value) % MOD;
    }
    let mut recent = Vec::new();
    let mut slot = cache.next[HEAD];
    while slot != TAIL && recent.len() < 5 {
        recent.push(cache.keys[slot].clone());
        slot = cache.next[slot];
    }
    println!(
        "lru_cache ops={n} hits={hits} misses={misses} evictions={} size={} checksum={checksum} recent={}",
        cache.evictions,
        cache.index.len(),
        recent.join(",")
    );
}
