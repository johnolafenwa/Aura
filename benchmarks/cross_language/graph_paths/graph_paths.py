"""Graph algorithms: BFS, Dijkstra with a binary heap, and union-find.

Builds a random weighted directed graph with N nodes and 4N extra edges on top
of a ring, then runs breadth-first search and Dijkstra's shortest paths from
node 0, and counts connected components of the undirected random edges. The binary
heap is written out, because Aura has no heap in its standard library.
"""
import sys


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def heap_push(heap, dist, node):
    heap.append((dist, node))
    i = len(heap) - 1
    while i > 0:
        parent = (i - 1) // 2
        if heap[parent] <= heap[i]:
            break
        heap[parent], heap[i] = heap[i], heap[parent]
        i = parent


def heap_pop(heap):
    top = heap[0]
    last = heap.pop()
    if heap:
        heap[0] = last
        i = 0
        size = len(heap)
        while True:
            left = 2 * i + 1
            right = left + 1
            smallest = i
            if left < size and heap[left] < heap[smallest]:
                smallest = left
            if right < size and heap[right] < heap[smallest]:
                smallest = right
            if smallest == i:
                break
            heap[smallest], heap[i] = heap[i], heap[smallest]
            i = smallest
    return top


def find(parent, x):
    while parent[x] != x:
        parent[x] = parent[parent[x]]
        x = parent[x]
    return x


def main():
    n = int(sys.argv[1])
    rng = Rng(23)
    adjacency = [[] for _ in range(n)]
    edges = []
    for i in range(n):
        edges.append((i, (i + 1) % n, 50))
    for _ in range(4 * n):
        u = rng.next() % n
        v = rng.next() % n
        w = 1 + rng.next() % 100
        edges.append((u, v, w))
    for u, v, w in edges:
        adjacency[u].append((v, w))

    hops = [-1] * n
    hops[0] = 0
    queue = [0]
    head = 0
    while head < len(queue):
        u = queue[head]
        head += 1
        for v, _ in adjacency[u]:
            if hops[v] < 0:
                hops[v] = hops[u] + 1
                queue.append(v)
    reachable = len(queue)
    hop_sum = sum(h for h in hops if h > 0)
    max_hops = max(hops)

    dist = [-1] * n
    dist[0] = 0
    heap = []
    heap_push(heap, 0, 0)
    pushes = 1
    settled = 0
    while heap:
        d, u = heap_pop(heap)
        if d > dist[u]:
            continue
        settled += 1
        for v, w in adjacency[u]:
            candidate = d + w
            if dist[v] < 0 or candidate < dist[v]:
                dist[v] = candidate
                heap_push(heap, candidate, v)
                pushes += 1
    dist_sum = sum(d for d in dist if d > 0)
    max_dist = max(dist)

    parent = list(range(n))
    size = [1] * n
    components = n
    for u, v, _ in edges[n:]:
        a = find(parent, u)
        b = find(parent, v)
        if a != b:
            if size[a] < size[b]:
                a, b = b, a
            parent[b] = a
            size[a] += size[b]
            components -= 1
    largest = max(size[find(parent, x)] for x in range(n))
    print(
        f"graph_paths nodes={n} edges={len(edges)} reachable={reachable} hops={hop_sum} "
        f"max_hops={max_hops} settled={settled} pushes={pushes} dist_sum={dist_sum} "
        f"max_dist={max_dist} components={components} largest={largest}"
    )


if __name__ == "__main__":
    main()
