use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let count: i64 = 8_192;
    let mut deque = VecDeque::new();
    let start = Instant::now();
    for i in 0..count {
        if i % 2 == 0 {
            deque.push_front(i);
        } else {
            deque.push_back(i);
        }
    }
    let sum: i64 = (0..count)
        .map(|i| {
            if i % 2 == 0 {
                deque.pop_front()
            } else {
                deque.pop_back()
            }
            .unwrap()
        })
        .sum();
    print_row("deque", start, sum);

    let mut heap = BinaryHeap::new();
    let start = Instant::now();
    for i in 0..count {
        heap.push(Reverse((i * 40503) % count));
    }
    let sum = (0..count).map(|i| heap.pop().unwrap().0 * i).sum();
    print_row("heap", start, sum);

    let mut map = BTreeMap::new();
    let start = Instant::now();
    for i in 0..count {
        map.insert((i * 40503) % count, i);
    }
    let sum = (0..count).map(|i| map[&i]).sum();
    print_row("map", start, sum);

    let mut set = BTreeSet::new();
    let start = Instant::now();
    for i in 0..count {
        set.insert((i * 40503) % count);
    }
    let sum = (0..count).filter(|i| set.remove(i)).sum();
    print_row("set", start, sum);
}

fn print_row(label: &str, start: Instant, value: i64) {
    let checksum = black_box(value);
    println!(
        "{label} {:.3} {checksum}",
        start.elapsed().as_secs_f64() * 1000.0
    );
}
