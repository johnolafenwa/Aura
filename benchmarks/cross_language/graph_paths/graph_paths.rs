//! Graph algorithms: BFS, Dijkstra with a binary heap, and union-find.
//!
//! Builds a random weighted directed graph with N nodes and 4N extra edges on top
//! of a ring, then runs breadth-first search and Dijkstra's shortest paths from
//! node 0, and counts connected components of the undirected random edges. The
//! binary heap is written out, because Aura has no heap in its standard library.

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

fn heap_push(heap: &mut Vec<(i64, usize)>, dist: i64, node: usize) {
    heap.push((dist, node));
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

fn heap_pop(heap: &mut Vec<(i64, usize)>) -> (i64, usize) {
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

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn main() {
    let n: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut rng = Rng { state: 23 };
    let mut edges: Vec<(usize, usize, i64)> = (0..n).map(|i| (i, (i + 1) % n, 50)).collect();
    for _ in 0..4 * n {
        let u = (rng.next() % n as i64) as usize;
        let v = (rng.next() % n as i64) as usize;
        let w = 1 + rng.next() % 100;
        edges.push((u, v, w));
    }
    let mut adjacency: Vec<Vec<(usize, i64)>> = vec![Vec::new(); n];
    for &(u, v, w) in &edges {
        adjacency[u].push((v, w));
    }

    let mut hops = vec![-1i64; n];
    hops[0] = 0;
    let mut queue = vec![0usize];
    let mut head = 0;
    while head < queue.len() {
        let u = queue[head];
        head += 1;
        for &(v, _) in &adjacency[u] {
            if hops[v] < 0 {
                hops[v] = hops[u] + 1;
                queue.push(v);
            }
        }
    }
    let reachable = queue.len();
    let hop_sum: i64 = hops.iter().filter(|&&h| h > 0).sum();
    let max_hops = *hops.iter().max().unwrap();

    let mut dist = vec![-1i64; n];
    dist[0] = 0;
    let mut heap = Vec::new();
    heap_push(&mut heap, 0, 0);
    let (mut pushes, mut settled) = (1, 0);
    while !heap.is_empty() {
        let (d, u) = heap_pop(&mut heap);
        if d > dist[u] {
            continue;
        }
        settled += 1;
        for &(v, w) in &adjacency[u] {
            let candidate = d + w;
            if dist[v] < 0 || candidate < dist[v] {
                dist[v] = candidate;
                heap_push(&mut heap, candidate, v);
                pushes += 1;
            }
        }
    }
    let dist_sum: i64 = dist.iter().filter(|&&d| d > 0).sum();
    let max_dist = *dist.iter().max().unwrap();

    let mut parent: Vec<usize> = (0..n).collect();
    let mut size = vec![1i64; n];
    let mut components = n;
    for &(u, v, _) in &edges[n..] {
        let mut a = find(&mut parent, u);
        let mut b = find(&mut parent, v);
        if a != b {
            if size[a] < size[b] {
                std::mem::swap(&mut a, &mut b);
            }
            parent[b] = a;
            size[a] += size[b];
            components -= 1;
        }
    }
    let largest = (0..n).map(|x| size[find(&mut parent, x)]).max().unwrap();
    println!(
        "graph_paths nodes={n} edges={} reachable={reachable} hops={hop_sum} max_hops={max_hops} settled={settled} \
         pushes={pushes} dist_sum={dist_sum} max_dist={max_dist} components={components} largest={largest}",
        edges.len()
    );
}
