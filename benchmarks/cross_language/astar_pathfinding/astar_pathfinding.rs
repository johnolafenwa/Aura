//! Game AI: A* path finding on a weighted grid.
//!
//! Builds an N x N grid where 28% of the cells are walls and the rest cost 1 to 3
//! to enter, then answers 12 path queries between random open cells with A*
//! (Manhattan heuristic, four neighbours, a hand-written binary heap of packed
//! (f, cell) keys). Visited marks are stamped per query, so no array is cleared.
//! Reports the reachable queries, the total path cost, and the cells expanded.

const QUERIES: i64 = 12;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

fn heap_push(heap: &mut Vec<i64>, key: i64) {
    heap.push(key);
    let mut i = heap.len() - 1;
    while i > 0 {
        let parent = (i - 1) / 2;
        if heap[parent] <= heap[i] {
            break;
        }
        heap.swap(parent, i);
        i = parent;
    }
}

fn heap_pop(heap: &mut Vec<i64>) -> i64 {
    let top = heap[0];
    let last = heap.pop().unwrap();
    if !heap.is_empty() {
        heap[0] = last;
        let size = heap.len();
        let mut i = 0;
        loop {
            let left = 2 * i + 1;
            let right = left + 1;
            let mut smallest = i;
            if left < size && heap[left] < heap[smallest] {
                smallest = left;
            }
            if right < size && heap[right] < heap[smallest] {
                smallest = right;
            }
            if smallest == i {
                break;
            }
            heap.swap(smallest, i);
            i = smallest;
        }
    }
    top
}

fn random_open_cell(rng: &mut Rng, grid: &[i64], cells: i64) -> i64 {
    loop {
        let cell = rng.next() % cells;
        if grid[cell as usize] > 0 {
            return cell;
        }
    }
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let cells = n * n;
    let mut rng = Rng { state: 67 };
    let mut grid = Vec::new();
    for _ in 0..cells {
        let r = rng.next() % 100;
        grid.push(if r < 28 { 0 } else { 1 + r % 3 });
    }
    let mut best = vec![0i64; cells as usize];
    let mut seen = vec![-1i64; cells as usize];
    let mut done = vec![-1i64; cells as usize];
    let (mut reachable, mut total_cost, mut expanded, mut checksum) = (0, 0i64, 0i64, 0i64);
    for query in 0..QUERIES {
        let start = random_open_cell(&mut rng, &grid, cells);
        let goal = random_open_cell(&mut rng, &grid, cells);
        let (gx, gy) = (goal % n, goal / n);
        best[start as usize] = 0;
        seen[start as usize] = query;
        let mut heap = vec![((start % n - gx).abs() + (start / n - gy).abs()) * cells + start];
        let mut cost = -1;
        while !heap.is_empty() {
            let key = heap_pop(&mut heap);
            let node = key % cells;
            if done[node as usize] == query {
                continue;
            }
            done[node as usize] = query;
            expanded += 1;
            let g = best[node as usize];
            if node == goal {
                cost = g;
                break;
            }
            let (x, y) = (node % n, node / n);
            for step in 0..4 {
                let neighbour = match step {
                    0 => {
                        if x + 1 >= n {
                            continue;
                        }
                        node + 1
                    }
                    1 => {
                        if x == 0 {
                            continue;
                        }
                        node - 1
                    }
                    2 => {
                        if y + 1 >= n {
                            continue;
                        }
                        node + n
                    }
                    _ => {
                        if y == 0 {
                            continue;
                        }
                        node - n
                    }
                };
                let weight = grid[neighbour as usize];
                if weight == 0 || done[neighbour as usize] == query {
                    continue;
                }
                let candidate = g + weight;
                if seen[neighbour as usize] != query || candidate < best[neighbour as usize] {
                    seen[neighbour as usize] = query;
                    best[neighbour as usize] = candidate;
                    let h = (neighbour % n - gx).abs() + (neighbour / n - gy).abs();
                    heap_push(&mut heap, (candidate + h) * cells + neighbour);
                }
            }
        }
        if cost >= 0 {
            reachable += 1;
            total_cost += cost;
        }
        checksum = (checksum * 31 + cost + 7 * expanded) % 1000000007;
    }
    println!(
        "astar_pathfinding grid={n} queries={QUERIES} reachable={reachable} total_cost={total_cost} \
         expanded={expanded} checksum={checksum}"
    );
}
