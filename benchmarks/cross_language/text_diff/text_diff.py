"""Developer tools: a line diff with the Myers O(ND) algorithm.

Generates a text of N lines and an edited copy (about 4% of lines deleted, 4%
inserted, 4% changed), interns every distinct line as an integer, finds a
shortest edit script with Myers' greedy algorithm (keeping one snapshot of the
frontier per edit distance for the backtrack), and renders it as diff lines
prefixed with ' ', '-', or '+'. Reports the edit distance, the operations, the
hunks, and a checksum of the rendered diff.
"""
import sys

WORDS = ["let", "value", "return", "if", "else", "for", "item", "count", "total", "name", "}", "{"]
KEEP = 0
DELETE = 1
INSERT = 2


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def make_line(rng):
    r = rng.next()
    return WORDS[r % 12] + " " + WORDS[(r // 12) % 12] + " " + str((r // 144) % 60)


def intern(lines, table):
    ids = []
    for line in lines:
        found = table.get(line)
        if found is None:
            found = len(table)
            table[line] = found
        ids.append(found)
    return ids


def myers(a, b):
    n = len(a)
    m = len(b)
    limit = n + m
    offset = limit + 1
    v = [0] * (2 * limit + 3)
    trace = []
    for d in range(limit + 1):
        trace.append(v[offset - d:offset + d + 1])
        for k in range(-d, d + 1, 2):
            if k == -d or (k != d and v[offset + k - 1] < v[offset + k + 1]):
                x = v[offset + k + 1]
            else:
                x = v[offset + k - 1] + 1
            y = x - k
            while x < n and y < m and a[x] == b[y]:
                x += 1
                y += 1
            v[offset + k] = x
            if x >= n and y >= m:
                return backtrack(trace, n, m)
    return []


def backtrack(trace, n, m):
    ops = []
    x = n
    y = m
    for d in range(len(trace) - 1, -1, -1):
        snapshot = trace[d]
        k = x - y
        if k == -d or (k != d and snapshot[k - 1 + d] < snapshot[k + 1 + d]):
            previous_k = k + 1
        else:
            previous_k = k - 1
        previous_x = snapshot[previous_k + d] if d > 0 else 0
        previous_y = previous_x - previous_k
        while x > previous_x and y > previous_y:
            x -= 1
            y -= 1
            ops.append(KEEP)
        if d > 0:
            if x == previous_x:
                ops.append(INSERT)
            else:
                ops.append(DELETE)
        x = previous_x
        y = previous_y
    ops.reverse()
    return ops


def main():
    n = int(sys.argv[1])
    rng = Rng(73)
    old = [make_line(rng) for _ in range(n)]
    new = []
    for line in old:
        r = rng.next() % 100
        if r < 4:
            continue
        if r < 8:
            new.append(make_line(rng))
        if r >= 8 and r < 12:
            new.append(make_line(rng))
            continue
        new.append(line)
    table = {}
    a = intern(old, table)
    b = intern(new, table)
    ops = myers(a, b)
    out = []
    i = 0
    j = 0
    hunks = 0
    previous = KEEP
    counts = [0, 0, 0]
    for op in ops:
        counts[op] += 1
        if op != KEEP and previous == KEEP:
            hunks += 1
        previous = op
        if op == KEEP:
            out.append(" " + old[i])
            i += 1
            j += 1
        elif op == DELETE:
            out.append("-" + old[i])
            i += 1
        else:
            out.append("+" + new[j])
            j += 1
    text = "\n".join(out)
    checksum = 0
    for line in out:
        checksum = (checksum * 31 + len(line)) % 1000000007
    print(
        f"text_diff old={n} new={len(new)} distinct={len(table)} distance={counts[1] + counts[2]} "
        f"kept={counts[0]} deleted={counts[1]} inserted={counts[2]} hunks={hunks} bytes={len(text)} checksum={checksum}"
    )


if __name__ == "__main__":
    main()
