"""Office software: a spreadsheet with formulas, a dependency graph, and recalculation.

Builds a sheet of N rows by 8 columns where about half the cells hold numbers
and the rest hold formulas such as =B12+C9*3, =SUM(D4:D13), or
=MAX(A2:A8)-MIN(A2:A8) that refer to earlier rows. It tokenizes and parses every
formula into a syntax tree, links each referenced cell to its dependents, orders
the cells topologically (Kahn's algorithm), evaluates them, then applies N / 4
edits to number cells, recomputing only the cells downstream of each edit.
Values wrap at 1,000,003 so they stay integers.
"""
import sys

COLS = 8
LETTERS = "ABCDEFGH"
MOD = 1000003
NUMBER = 0
CELL = 1
FUNCTION = 2
SYMBOL = 3
END = 4
SUM = 0
MAX = 1
MIN = 2


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def wrap(value):
    return (value % MOD + MOD) % MOD


def tokenize(text):
    data = text.encode()
    kinds = []
    values = []
    i = 0
    while i < len(data):
        c = data[i]
        if 48 <= c <= 57:
            value = 0
            while i < len(data) and 48 <= data[i] <= 57:
                value = value * 10 + data[i] - 48
                i += 1
            kinds.append(NUMBER)
            values.append(value)
        elif 65 <= c <= 90:
            start = i
            while i < len(data) and 65 <= data[i] <= 90:
                i += 1
            if i < len(data) and 48 <= data[i] <= 57:
                row = 0
                while i < len(data) and 48 <= data[i] <= 57:
                    row = row * 10 + data[i] - 48
                    i += 1
                kinds.append(CELL)
                values.append((row - 1) * COLS + (data[start] - 65))
            else:
                kinds.append(FUNCTION)
                if data[start] == 83:
                    values.append(SUM)
                elif data[start + 1] == 65:
                    values.append(MAX)
                else:
                    values.append(MIN)
        else:
            if c != 32:
                kinds.append(SYMBOL)
                values.append(c)
            i += 1
    kinds.append(END)
    values.append(0)
    return kinds, values


class Parser:
    def __init__(self, kinds, values):
        self.kinds = kinds
        self.values = values
        self.pos = 0
        self.refs = []

    def symbol_is(self, code):
        return self.kinds[self.pos] == SYMBOL and self.values[self.pos] == code

    def expr(self):
        left = self.term()
        while self.symbol_is(43) or self.symbol_is(45):
            op = self.values[self.pos]
            self.pos += 1
            left = (4, op, left, self.term())
        return left

    def term(self):
        left = self.factor()
        while self.symbol_is(42):
            self.pos += 1
            left = (4, 42, left, self.factor())
        return left

    def factor(self):
        kind = self.kinds[self.pos]
        value = self.values[self.pos]
        self.pos += 1
        if kind == NUMBER:
            return (0, value)
        if kind == CELL:
            self.refs.append(value)
            return (1, value)
        if kind == FUNCTION:
            self.pos += 1
            first = self.values[self.pos]
            self.pos += 2
            last = self.values[self.pos]
            self.pos += 2
            for cell in range(first, last + 1, COLS):
                self.refs.append(cell)
            return (2, value, first, last)
        if value == 45:
            return (3, self.factor())
        inner = self.expr()
        self.pos += 1
        return inner


def evaluate(node, values):
    tag = node[0]
    if tag == 0:
        return node[1]
    if tag == 1:
        return values[node[1]]
    if tag == 2:
        result = 0 if node[1] == SUM else values[node[2]]
        for cell in range(node[2], node[3] + 1, COLS):
            if node[1] == SUM:
                result = wrap(result + values[cell])
            elif node[1] == MAX:
                result = max(result, values[cell])
            else:
                result = min(result, values[cell])
        return result
    if tag == 3:
        return wrap(-evaluate(node[1], values))
    left = evaluate(node[2], values)
    right = evaluate(node[3], values)
    if node[1] == 43:
        return wrap(left + right)
    if node[1] == 45:
        return wrap(left - right)
    return wrap(left * right)


def cell_name(cell):
    return LETTERS[cell % COLS] + str(cell // COLS + 1)


def make_formula(rng, row, col):
    r = rng.next()
    low = max(0, row - 12)

    def pick(salt):
        return ((low + salt % (row - low)) * COLS) + (salt // 7) % COLS

    kind = r % 4
    if kind == 0:
        return "=" + cell_name(pick(r // 4)) + "+" + cell_name(pick(r // 97)) + "*" + str(r % 9 + 1)
    first_row = low + (r // 4) % (row - low)
    last_row = min(row - 1, first_row + 9)
    column = (r // 13) % COLS
    span = LETTERS[column] + str(first_row + 1) + ":" + LETTERS[column] + str(last_row + 1)
    if kind == 1:
        return "=SUM(" + span + ")"
    if kind == 2:
        return "=MAX(" + span + ")-MIN(" + span + ")"
    return "=(" + cell_name(pick(r // 5)) + "-" + cell_name(pick(r // 211)) + ")*" + str(r % 5 + 2) + "+7"


def main():
    n = int(sys.argv[1])
    cells = n * COLS
    rng = Rng(79)
    values = [0] * cells
    formulas = [None] * cells
    dependents = [[] for _ in range(cells)]
    indegree = [0] * cells
    formula_count = 0
    formula_bytes = 0
    for row in range(n):
        for col in range(COLS):
            cell = row * COLS + col
            r = rng.next()
            if row == 0 or r % 2 == 0:
                values[cell] = r % 1000
                continue
            text = make_formula(rng, row, col)
            formula_bytes += len(text)
            kinds, token_values = tokenize(text[1:])
            parser = Parser(kinds, token_values)
            formulas[cell] = parser.expr()
            formula_count += 1
            for ref in parser.refs:
                dependents[ref].append(cell)
                indegree[cell] += 1
    order = []
    for cell in range(cells):
        if indegree[cell] == 0:
            order.append(cell)
    head = 0
    while head < len(order):
        cell = order[head]
        head += 1
        for dependent in dependents[cell]:
            indegree[dependent] -= 1
            if indegree[dependent] == 0:
                order.append(dependent)
    for cell in order:
        if formulas[cell] is not None:
            values[cell] = evaluate(formulas[cell], values)
    first_total = 0
    for value in values:
        first_total = (first_total + value) % MOD

    dirty = [-1] * cells
    recomputed = 0
    edits = n // 4
    for edit in range(edits):
        target = rng.next() % cells
        while formulas[target] is not None:
            target = (target + 1) % cells
        values[target] = rng.next() % 1000
        stack = [target]
        dirty[target] = edit
        while stack:
            cell = stack.pop()
            for dependent in dependents[cell]:
                if dirty[dependent] != edit:
                    dirty[dependent] = edit
                    stack.append(dependent)
        for cell in order:
            if dirty[cell] == edit and formulas[cell] is not None:
                values[cell] = evaluate(formulas[cell], values)
                recomputed += 1
    total = 0
    checksum = 0
    for value in values:
        total = (total + value) % MOD
        checksum = (checksum * 31 + value) % 1000000007
    print(
        f"spreadsheet cells={cells} formulas={formula_count} formula_bytes={formula_bytes} ordered={len(order)} "
        f"first_total={first_total} edits={edits} recomputed={recomputed} total={total} checksum={checksum}"
    )


if __name__ == "__main__":
    main()
