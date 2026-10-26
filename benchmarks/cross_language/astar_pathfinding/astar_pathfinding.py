"""Game AI: A* path finding on a weighted grid.

Builds an N x N grid where 28% of the cells are walls and the rest cost 1 to 3
to enter, then answers 12 path queries between random open cells with A*
(Manhattan heuristic, four neighbours, a hand-written binary heap of packed
(f, cell) keys). Visited marks are stamped per query, so no array is cleared.
Reports the reachable queries, the total path cost, and the cells expanded.
"""
import sys

QUERIES = 12


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def heap_push(heap, key):
    heap.append(key)
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
        size = len(heap)
        i = 0
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


def random_open_cell(rng, grid, cells):
    while True:
        cell = rng.next() % cells
        if grid[cell] > 0:
            return cell


def main():
    n = int(sys.argv[1])
    cells = n * n
    rng = Rng(67)
    grid = []
    for _ in range(cells):
        r = rng.next() % 100
        grid.append(0 if r < 28 else 1 + r % 3)
    best = [0] * cells
    seen = [-1] * cells
    done = [-1] * cells
    reachable = 0
    total_cost = 0
    expanded = 0
    checksum = 0
    for query in range(QUERIES):
        start = random_open_cell(rng, grid, cells)
        goal = random_open_cell(rng, grid, cells)
        gx = goal % n
        gy = goal // n
        best[start] = 0
        seen[start] = query
        heap = [(abs(start % n - gx) + abs(start // n - gy)) * cells + start]
        cost = -1
        while heap:
            key = heap_pop(heap)
            node = key % cells
            if done[node] == query:
                continue
            done[node] = query
            expanded += 1
            g = best[node]
            if node == goal:
                cost = g
                break
            x = node % n
            y = node // n
            for step in range(4):
                if step == 0:
                    if x + 1 >= n:
                        continue
                    neighbour = node + 1
                elif step == 1:
                    if x == 0:
                        continue
                    neighbour = node - 1
                elif step == 2:
                    if y + 1 >= n:
                        continue
                    neighbour = node + n
                else:
                    if y == 0:
                        continue
                    neighbour = node - n
                weight = grid[neighbour]
                if weight == 0 or done[neighbour] == query:
                    continue
                candidate = g + weight
                if seen[neighbour] != query or candidate < best[neighbour]:
                    seen[neighbour] = query
                    best[neighbour] = candidate
                    h = abs(neighbour % n - gx) + abs(neighbour // n - gy)
                    heap_push(heap, (candidate + h) * cells + neighbour)
        if cost >= 0:
            reachable += 1
            total_cost += cost
        checksum = (checksum * 31 + cost + 7 * expanded) % 1000000007
    print(
        f"astar_pathfinding grid={n} queries={QUERIES} reachable={reachable} total_cost={total_cost} "
        f"expanded={expanded} checksum={checksum}"
    )


if __name__ == "__main__":
    main()
