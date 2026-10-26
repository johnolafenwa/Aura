"""Text search: a small regular-expression engine run over generated log lines.

Compiles ten patterns (literals, `.`, `*`, `+`, `?`, `^`, `$`, `[...]`
classes, `\\d`, `\\w`, `\\s`) into a token program with one 256-entry byte
table per token, then runs each pattern over N log lines with a Thompson NFA
simulation (a Pike VM without captures). It counts which lines each pattern
matches. The engine is written out so all three languages do the same work.
"""
import sys

ONE = 0
OPTIONAL = 1
STAR = 2

PATTERNS = [
    "ERROR",
    "^\\d+-\\d+-\\d+ \\d+:\\d+:\\d+ WARN",
    "user=[a-z]+\\d+ ",
    "latency=\\d\\d\\d\\d+ms",
    "GET /api/v\\d/items/\\d+ ok$",
    "[A-Z][a-z]+Exception$",
    "id=[0-9a-f]+-[0-9a-f]+",
    "^.*timeout.*retry \\d$",
    "\\s[a-z]+\\s[a-z]+\\s[a-z]+$",
    "co?u?nn?ection re[a-z]*t",
]
LEVELS = ["INFO", "WARN", "ERROR", "DEBUG"]
COMPONENTS = ["auth", "db", "cache", "api", "queue"]
NAMES = ["alice", "bob", "carol", "dave", "erin", "frank"]
HEX_DIGITS = "0123456789abcdef"


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


class Program:
    def __init__(self):
        self.tables = []
        self.quants = []
        self.count = 0
        self.anchored_start = False
        self.anchored_end = False


def to_hex(value):
    if value == 0:
        return "0"
    digits = []
    while value > 0:
        digits.append(HEX_DIGITS[value % 16])
        value //= 16
    digits.reverse()
    return "".join(digits)


def pad2(value):
    if value < 10:
        return "0" + str(value)
    return str(value)


def make_line(rng):
    r1 = rng.next()
    r2 = rng.next()
    r3 = rng.next()
    day = 1 + r1 % 28
    hour = (r1 // 28) % 24
    minute = (r1 // 672) % 60
    second = r2 % 60
    level = LEVELS[(r2 // 60) % 4]
    component = COMPONENTS[(r2 // 240) % 5]
    name = NAMES[r3 % 6]
    if r3 % 3 == 0:
        name += str((r3 // 6) % 100)
    latency = (r3 // 7) % 2000
    kind = (r3 // 14000) % 6
    a = 1 + r1 % 3
    b = r2 % 10000
    if kind == 0:
        message = "GET /api/v" + str(a) + "/items/" + str(b) + " ok"
    elif kind == 1:
        message = "timeout after " + str(b) + "ms, retry " + str(a)
    elif kind == 2:
        message = "cache miss for id=" + to_hex(b * 7919) + "-" + to_hex(r3 % 65536)
    elif kind == 3:
        message = "raised " + ["Value", "Key", "Io"][a - 1] + "Exception"
    elif kind == 4:
        message = "POST /api/v" + str(a) + "/orders/" + str(b) + " created"
    else:
        message = "connection reset by peer"
    return (
        "2026-10-" + pad2(day) + " " + pad2(hour) + ":" + pad2(minute) + ":" + pad2(second)
        + " " + level + " " + component + " user=" + name + " latency=" + str(latency)
        + "ms " + message
    )


def class_table(program):
    base = program.count * 256
    program.tables.extend([False] * 256)
    program.count += 1
    return base


def set_range(program, base, low, high):
    for b in range(low, high + 1):
        program.tables[base + b] = True


def compile_pattern(pattern):
    p = pattern.encode()
    program = Program()
    i = 0
    if p[0] == ord("^"):
        program.anchored_start = True
        i = 1
    while i < len(p):
        c = p[i]
        if c == ord("$") and i == len(p) - 1:
            program.anchored_end = True
            break
        base = class_table(program)
        if c == ord("."):
            set_range(program, base, 0, 255)
            program.tables[base + ord("\n")] = False
        elif c == ord("\\"):
            i += 1
            e = p[i]
            if e == ord("d"):
                set_range(program, base, ord("0"), ord("9"))
            elif e == ord("w"):
                set_range(program, base, ord("0"), ord("9"))
                set_range(program, base, ord("a"), ord("z"))
                set_range(program, base, ord("A"), ord("Z"))
                program.tables[base + ord("_")] = True
            elif e == ord("s"):
                program.tables[base + ord(" ")] = True
                program.tables[base + ord("\t")] = True
            else:
                program.tables[base + e] = True
        elif c == ord("["):
            i += 1
            while p[i] != ord("]"):
                if p[i + 1] == ord("-") and p[i + 2] != ord("]"):
                    set_range(program, base, p[i], p[i + 2])
                    i += 3
                else:
                    program.tables[base + p[i]] = True
                    i += 1
        else:
            program.tables[base + c] = True
        i += 1
        quant = ONE
        if i < len(p):
            if p[i] == ord("?"):
                quant = OPTIONAL
                i += 1
            elif p[i] == ord("*"):
                quant = STAR
                i += 1
            elif p[i] == ord("+"):
                # x+ is x followed by x*.
                program.quants.append(ONE)
                copy = class_table(program)
                for b in range(256):
                    program.tables[copy + b] = program.tables[base + b]
                quant = STAR
                i += 1
        program.quants.append(quant)
    return program


class Matcher:
    """Visited marks shared by every pattern; the generation only grows."""

    def __init__(self, size):
        self.marks = [0] * size
        self.generation = 0

    def add_state(self, program, states, start):
        s = start
        while self.marks[s] != self.generation:
            self.marks[s] = self.generation
            states.append(s)
            if s == program.count or program.quants[s] == ONE:
                return
            s += 1

    def search(self, program, data):
        accept = program.count
        self.generation += 1
        current = []
        self.add_state(program, current, 0)
        pos = 0
        while True:
            if self.marks[accept] == self.generation and (not program.anchored_end or pos == len(data)):
                return True
            if pos == len(data) or not current:
                return False
            byte = data[pos]
            self.generation += 1
            following = []
            for s in current:
                if s < accept and program.tables[s * 256 + byte]:
                    if program.quants[s] == STAR:
                        self.add_state(program, following, s)
                    else:
                        self.add_state(program, following, s + 1)
            pos += 1
            if not program.anchored_start:
                self.add_state(program, following, 0)
            current = following


def main():
    n = int(sys.argv[1])
    rng = Rng(31)
    programs = [compile_pattern(pattern) for pattern in PATTERNS]
    matcher = Matcher(max(program.count for program in programs) + 1)
    counts = [0] * len(programs)
    checksum = 0
    total_bytes = 0
    for line_number in range(n):
        data = make_line(rng).encode()
        total_bytes += len(data)
        mask = 0
        for k in range(len(programs)):
            if matcher.search(programs[k], data):
                counts[k] += 1
                mask |= 1 << k
        checksum = (checksum * 31 + mask * (line_number % 13 + 1)) % 1000000007
    tokens = sum(program.count for program in programs)
    print(
        f"regex_match lines={n} bytes={total_bytes} tokens={tokens} counts="
        + ",".join(str(c) for c in counts)
        + f" checksum={checksum}"
    )


if __name__ == "__main__":
    main()
