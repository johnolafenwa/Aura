"""Databases: a small SQL engine with a tokenizer, parser, planner, and executor.

Runs N INSERT statements into a users table, then N more statements mixing
point SELECTs, UPDATEs and DELETEs by id (planned onto a hash index), new
INSERTs, filtered range scans, and GROUP BY aggregates. Every statement arrives
as SQL text, so each one is tokenized, parsed into a statement value, and
executed against row storage with an id index. Reports the rows affected and
a checksum of everything the SELECTs returned.
"""
import sys

CITIES = ["paris", "lagos", "tokyo", "lima", "oslo", "cairo", "quito", "perth"]
SYLLABLES = ["ka", "lo", "mi", "ra", "to", "ne", "su", "vi"]
WORD = 0
NUMBER = 1
TEXT = 2
SYMBOL = 3
END = 4
EQ = 0
NE = 1
LT = 2
LE = 3
GT = 4
GE = 5
COLUMN = 0
COUNT = 1
SUM = 2
MAX = 3
MOD = 1000000007


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def tokenize(sql):
    data = sql.encode()
    kinds = []
    texts = []
    numbers = []
    i = 0
    n = len(data)
    while i < n:
        c = data[i]
        if c == 32:
            i += 1
        elif 48 <= c <= 57:
            value = 0
            while i < n and 48 <= data[i] <= 57:
                value = value * 10 + data[i] - 48
                i += 1
            kinds.append(NUMBER)
            texts.append("")
            numbers.append(value)
        elif c == 39:
            start = i + 1
            i = start
            while data[i] != 39:
                i += 1
            kinds.append(TEXT)
            texts.append(data[start:i].decode())
            numbers.append(0)
            i += 1
        elif (65 <= c <= 90) or (97 <= c <= 122) or c == 95:
            start = i
            while i < n and ((65 <= data[i] <= 90) or (97 <= data[i] <= 122) or data[i] == 95):
                i += 1
            kinds.append(WORD)
            texts.append(data[start:i].decode())
            numbers.append(0)
        else:
            if (c == 60 or c == 62 or c == 33) and i + 1 < n and data[i + 1] == 61:
                kinds.append(SYMBOL)
                texts.append(data[i:i + 2].decode())
                i += 2
            else:
                kinds.append(SYMBOL)
                texts.append(data[i:i + 1].decode())
                i += 1
            numbers.append(0)
    kinds.append(END)
    texts.append("")
    numbers.append(0)
    return kinds, texts, numbers


class Parser:
    def __init__(self, sql, columns):
        self.kinds, self.texts, self.numbers = tokenize(sql)
        self.pos = 0
        self.columns = columns

    def word(self):
        text = self.texts[self.pos]
        self.pos += 1
        return text

    def skip(self):
        self.pos += 1

    def column(self):
        return self.columns[self.word()]

    def literal(self):
        kind = self.kinds[self.pos]
        self.pos += 1
        if kind == NUMBER:
            return self.numbers[self.pos - 1]
        return self.texts[self.pos - 1]

    def conditions(self):
        result = []
        if self.kinds[self.pos] == WORD and self.texts[self.pos] == "WHERE":
            self.skip()
            while True:
                column = self.column()
                op = ["=", "!=", "<", "<=", ">", ">="].index(self.word())
                result.append((column, op, self.literal()))
                if self.kinds[self.pos] == WORD and self.texts[self.pos] == "AND":
                    self.skip()
                else:
                    break
        return result

    def statement(self):
        verb = self.word()
        if verb == "INSERT":
            self.pos += 4
            values = [self.literal()]
            while self.texts[self.pos] == ",":
                self.skip()
                values.append(self.literal())
            return ("insert", values)
        if verb == "SELECT":
            items = []
            while True:
                name = self.word()
                if name == "COUNT":
                    self.pos += 3
                    items.append((COUNT, 0))
                elif name == "SUM" or name == "MAX":
                    self.skip()
                    column = self.column()
                    self.skip()
                    items.append((SUM if name == "SUM" else MAX, column))
                else:
                    items.append((COLUMN, self.columns[name]))
                if self.texts[self.pos] == ",":
                    self.skip()
                else:
                    break
            self.pos += 2
            where = self.conditions()
            group = -1
            if self.kinds[self.pos] == WORD and self.texts[self.pos] == "GROUP":
                self.pos += 2
                group = self.column()
            return ("select", items, where, group)
        if verb == "UPDATE":
            self.pos += 2
            target = self.column()
            self.skip()
            if self.kinds[self.pos] == WORD:
                source = self.column()
                self.skip()
                delta = self.literal()
            else:
                source = -1
                delta = self.literal()
            return ("update", target, source, delta, self.conditions())
        self.pos += 2
        return ("delete", self.conditions())


def matches(row, where):
    for column, op, value in where:
        cell = row[column]
        if op == EQ:
            ok = cell == value
        elif op == NE:
            ok = cell != value
        elif op == LT:
            ok = cell < value
        elif op == LE:
            ok = cell <= value
        elif op == GT:
            ok = cell > value
        else:
            ok = cell >= value
        if not ok:
            return False
    return True


def value_hash(value):
    if isinstance(value, int):
        return value
    return len(value) * 131 + ord(value[0])


class Engine:
    def __init__(self):
        self.columns = {"id": 0, "name": 1, "age": 2, "city": 3}
        self.rows = []
        self.live = []
        self.index = {}
        self.inserted = 0
        self.selected = 0
        self.groups = 0
        self.updated = 0
        self.deleted = 0
        self.scans = 0
        self.checksum = 0

    def candidates(self, where):
        # The planner: an equality on id uses the index, anything else scans.
        for column, op, value in where:
            if column == 0 and op == EQ:
                slot = self.index.get(value)
                return [] if slot is None else [slot]
        self.scans += 1
        return [slot for slot in range(len(self.rows)) if self.live[slot]]

    def execute(self, sql):
        statement = Parser(sql, self.columns).statement()
        kind = statement[0]
        if kind == "insert":
            self.index[statement[1][0]] = len(self.rows)
            self.rows.append(statement[1])
            self.live.append(True)
            self.inserted += 1
        elif kind == "select":
            _, items, where, group = statement
            if group < 0:
                for slot in self.candidates(where):
                    row = self.rows[slot]
                    if matches(row, where):
                        self.selected += 1
                        for item_kind, column in items:
                            self.checksum = (self.checksum * 31 + value_hash(row[column])) % MOD
                return
            keys = []
            totals = {}
            for slot in self.candidates(where):
                row = self.rows[slot]
                if not matches(row, where):
                    continue
                key = row[group]
                state = totals.get(key)
                if state is None:
                    state = [0] * len(items)
                    totals[key] = state
                    keys.append(key)
                for i in range(len(items)):
                    item_kind, column = items[i]
                    if item_kind == COUNT:
                        state[i] += 1
                    elif item_kind == SUM:
                        state[i] += row[column]
                    elif item_kind == MAX:
                        state[i] = max(state[i], row[column])
            for key in keys:
                self.groups += 1
                for value in totals[key]:
                    self.checksum = (self.checksum + value_hash(key) * value) % MOD
        elif kind == "update":
            _, target, source, delta, where = statement
            for slot in self.candidates(where):
                row = self.rows[slot]
                if matches(row, where):
                    row[target] = row[source] + delta if source >= 0 else delta
                    self.updated += 1
        else:
            for slot in self.candidates(statement[1]):
                row = self.rows[slot]
                if matches(row, statement[1]):
                    self.live[slot] = False
                    del self.index[row[0]]
                    self.deleted += 1


def insert_sql(rng, row_id):
    r = rng.next()
    name = SYLLABLES[r % 8] + SYLLABLES[(r // 8) % 8] + SYLLABLES[(r // 64) % 8]
    return (
        "INSERT INTO users VALUES (" + str(row_id) + ", '" + name + "', " + str(18 + (r // 512) % 60)
        + ", '" + CITIES[(r // 30720) % 8] + "')"
    )


def main():
    n = int(sys.argv[1])
    rng = Rng(89)
    engine = Engine()
    for row_id in range(n):
        engine.execute(insert_sql(rng, row_id))
    next_id = n
    for _ in range(n):
        r = rng.next()
        kind = r % 1000
        target = (r // 1000) % next_id
        if kind < 500:
            engine.execute("SELECT name, age, city FROM users WHERE id = " + str(target))
        elif kind < 700:
            engine.execute("UPDATE users SET age = age + 1 WHERE id = " + str(target))
        elif kind < 800:
            engine.execute("DELETE FROM users WHERE id = " + str(target))
        elif kind < 993:
            engine.execute(insert_sql(rng, next_id))
            next_id += 1
        elif kind < 998:
            low = 18 + (r // 1000) % 60
            engine.execute(
                "SELECT id, name FROM users WHERE age >= " + str(low) + " AND age < " + str(low + 3)
                + " AND city = '" + CITIES[(r // 7) % 8] + "'"
            )
        else:
            engine.execute("SELECT city, COUNT(*), SUM(age), MAX(age) FROM users WHERE age > 20 GROUP BY city")
    print(
        f"sql_engine statements={2 * n} inserted={engine.inserted} selected={engine.selected} groups={engine.groups} "
        f"updated={engine.updated} deleted={engine.deleted} scans={engine.scans} live={len(engine.index)} "
        f"checksum={engine.checksum}"
    )


if __name__ == "__main__":
    main()
