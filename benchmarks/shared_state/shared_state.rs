// Shared state benchmark, the Rust side. See shared_state.au.
use std::collections::HashMap;
use std::hint::black_box;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

const LOCK_STEPS: i64 = 4_000_000;
const ATOMIC_STEPS: i64 = 20_000_000;
const DICT_STEPS: i64 = 4_000_000;
const CACHE_STEPS: i64 = 1_000_000;
const CACHE_KEYS: i64 = 1024;
const TASKS: i64 = 4;

fn lock_steps(counter: &Mutex<i64>, steps: i64) {
    for _ in 0..steps {
        *black_box(counter).lock().unwrap() += 1;
    }
}

fn atomic_steps(counter: &AtomicI64, steps: i64) {
    for _ in 0..steps {
        black_box(counter).fetch_add(1, Ordering::SeqCst);
    }
}

fn dict_steps(table: &Mutex<HashMap<i64, i64>>, steps: i64) {
    for i in 0..steps {
        table.lock().unwrap().insert(i % CACHE_KEYS, i);
    }
}

fn cache_steps(cache: &Mutex<HashMap<i64, i64>>, seed: i64, steps: i64) -> i64 {
    let mut state = seed;
    let mut total = 0;
    for _ in 0..steps {
        state = (state * 48271) % 2147483647;
        let key = state % CACHE_KEYS;
        let found = cache.lock().unwrap().get(&key).copied();
        match found {
            Some(known) => total += known,
            None => {
                cache.lock().unwrap().insert(key, key * 3);
                total += key * 3;
            }
        }
    }
    total
}

fn main() {
    let counter = Mutex::new(0i64);
    let start = Instant::now();
    lock_steps(&counter, LOCK_STEPS);
    println!("lock {} {}", start.elapsed().as_millis(), counter.lock().unwrap());

    let counter = AtomicI64::new(0);
    let start = Instant::now();
    atomic_steps(&counter, ATOMIC_STEPS);
    println!("atomic {} {}", start.elapsed().as_millis(), counter.load(Ordering::SeqCst));

    let table = Mutex::new(HashMap::new());
    let start = Instant::now();
    dict_steps(&table, DICT_STEPS);
    println!("dict {} {}", start.elapsed().as_millis(), table.lock().unwrap().len());

    let counter = Arc::new(Mutex::new(0i64));
    let start = Instant::now();
    let workers: Vec<_> = (0..TASKS)
        .map(|_| {
            let counter = Arc::clone(&counter);
            thread::spawn(move || lock_steps(&counter, LOCK_STEPS / TASKS))
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    println!("lock_shared {} {}", start.elapsed().as_millis(), counter.lock().unwrap());

    let counter = Arc::new(AtomicI64::new(0));
    let start = Instant::now();
    let workers: Vec<_> = (0..TASKS)
        .map(|_| {
            let counter = Arc::clone(&counter);
            thread::spawn(move || atomic_steps(&counter, ATOMIC_STEPS / TASKS))
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    println!(
        "atomic_shared {} {}",
        start.elapsed().as_millis(),
        counter.load(Ordering::SeqCst)
    );

    let cache = Arc::new(Mutex::new(HashMap::new()));
    let start = Instant::now();
    let workers: Vec<_> = [11i64, 23, 37, 41]
        .into_iter()
        .map(|seed| {
            let cache = Arc::clone(&cache);
            thread::spawn(move || cache_steps(&cache, seed, CACHE_STEPS))
        })
        .collect();
    let total: i64 = workers.into_iter().map(|worker| worker.join().unwrap()).sum();
    println!(
        "cache_shared {} {} {}",
        start.elapsed().as_millis(),
        total,
        cache.lock().unwrap().len()
    );
}
