"""Tree-walking interpreter: lex, parse, and run a small imperative language.

The guest program sums Collatz sequence lengths for 1..N and counts primes
below N by trial division. Variables live in one string-keyed environment.
"""
import sys

SOURCE = """
let total = 0;
let i = 1;
while (i <= LIMIT) {
    let x = i;
    let steps = 0;
    while (x != 1) {
        if (x % 2 == 0) { x = x / 2; } else { x = 3 * x + 1; }
        steps = steps + 1;
    }
    total = total + steps;
    i = i + 1;
}
print total;
let primes = 0;
let k = 2;
while (k < LIMIT) {
    let d = 2;
    let prime = 1;
    while (d * d <= k && prime) {
        if (k % d == 0) { prime = 0; }
        d = d + 1;
    }
    primes = primes + prime;
    k = k + 1;
}
print primes;
print -(total % 1000) + !primes;
"""

NUMBER, NAME, SYMBOL, END = 0, 1, 2, 3
TWO_CHAR = ("==", "!=", "<=", ">=", "&&", "||")


def tokenize(source):
    tokens = []
    i = 0
    n = len(source)
    while i < n:
        c = source[i]
        if c in " \n\t\r":
            i += 1
        elif c.isdigit():
            start = i
            while i < n and source[i].isdigit():
                i += 1
            tokens.append((NUMBER, source[start:i]))
        elif c.isalpha() or c == "_":
            start = i
            while i < n and (source[i].isalnum() or source[i] == "_"):
                i += 1
            tokens.append((NAME, source[start:i]))
        elif source[i:i + 2] in TWO_CHAR:
            tokens.append((SYMBOL, source[i:i + 2]))
            i += 2
        else:
            tokens.append((SYMBOL, c))
            i += 1
    tokens.append((END, ""))
    return tokens


class Num:
    def __init__(self, value):
        self.value = value

    def eval(self, env):
        return self.value


class Var:
    def __init__(self, name):
        self.name = name

    def eval(self, env):
        return env[self.name]


class Unary:
    def __init__(self, op, operand):
        self.op = op
        self.operand = operand

    def eval(self, env):
        value = self.operand.eval(env)
        if self.op == "-":
            return -value
        return 1 if value == 0 else 0


class Binary:
    def __init__(self, op, left, right):
        self.op = op
        self.left = left
        self.right = right

    def eval(self, env):
        op = self.op
        if op == "&&":
            return 1 if self.left.eval(env) != 0 and self.right.eval(env) != 0 else 0
        if op == "||":
            return 1 if self.left.eval(env) != 0 or self.right.eval(env) != 0 else 0
        a = self.left.eval(env)
        b = self.right.eval(env)
        if op == "+":
            return a + b
        if op == "-":
            return a - b
        if op == "*":
            return a * b
        if op == "/":
            return a // b
        if op == "%":
            return a % b
        if op == "==":
            return 1 if a == b else 0
        if op == "!=":
            return 1 if a != b else 0
        if op == "<":
            return 1 if a < b else 0
        if op == "<=":
            return 1 if a <= b else 0
        if op == ">":
            return 1 if a > b else 0
        return 1 if a >= b else 0


class Assign:
    def __init__(self, name, value):
        self.name = name
        self.value = value

    def run(self, env, out):
        env[self.name] = self.value.eval(env)


class While:
    def __init__(self, condition, body):
        self.condition = condition
        self.body = body

    def run(self, env, out):
        while self.condition.eval(env) != 0:
            for statement in self.body:
                statement.run(env, out)


class If:
    def __init__(self, condition, then_body, else_body):
        self.condition = condition
        self.then_body = then_body
        self.else_body = else_body

    def run(self, env, out):
        body = self.then_body if self.condition.eval(env) != 0 else self.else_body
        for statement in body:
            statement.run(env, out)


class Print:
    def __init__(self, value):
        self.value = value

    def run(self, env, out):
        out.append(self.value.eval(env))


BINARY_LEVELS = [("||",), ("&&",), ("==", "!="), ("<", "<=", ">", ">="), ("+", "-"), ("*", "/", "%")]


class Parser:
    def __init__(self, tokens):
        self.tokens = tokens
        self.pos = 0
        self.nodes = 0

    def peek(self):
        return self.tokens[self.pos][1]

    def take(self):
        token = self.tokens[self.pos]
        self.pos += 1
        return token[1]

    def expect(self, text):
        if self.take() != text:
            raise SyntaxError(f"expected {text}")

    def program(self):
        statements = []
        while self.tokens[self.pos][0] != END:
            statements.append(self.statement())
        return statements

    def block(self):
        self.expect("{")
        statements = []
        while self.peek() != "}":
            statements.append(self.statement())
        self.expect("}")
        return statements

    def statement(self):
        self.nodes += 1
        word = self.take()
        if word == "let":
            name = self.take()
            self.expect("=")
            value = self.expression(0)
            self.expect(";")
            return Assign(name, value)
        if word == "while":
            self.expect("(")
            condition = self.expression(0)
            self.expect(")")
            return While(condition, self.block())
        if word == "if":
            self.expect("(")
            condition = self.expression(0)
            self.expect(")")
            then_body = self.block()
            else_body = []
            if self.peek() == "else":
                self.take()
                else_body = self.block()
            return If(condition, then_body, else_body)
        if word == "print":
            value = self.expression(0)
            self.expect(";")
            return Print(value)
        self.expect("=")
        value = self.expression(0)
        self.expect(";")
        return Assign(word, value)

    def expression(self, level):
        if level == len(BINARY_LEVELS):
            return self.unary()
        left = self.expression(level + 1)
        while self.peek() in BINARY_LEVELS[level]:
            op = self.take()
            right = self.expression(level + 1)
            self.nodes += 1
            left = Binary(op, left, right)
        return left

    def unary(self):
        self.nodes += 1
        if self.peek() in ("-", "!"):
            op = self.take()
            return Unary(op, self.unary())
        kind, text = self.tokens[self.pos]
        self.pos += 1
        if kind == NUMBER:
            return Num(int(text))
        if kind == NAME:
            return Var(text)
        value = self.expression(0)
        self.expect(")")
        return value


def main():
    limit = int(sys.argv[1])
    tokens = tokenize(SOURCE.replace("LIMIT", str(limit)))
    parser = Parser(tokens)
    program = parser.program()
    env = {}
    out = []
    for statement in program:
        statement.run(env, out)
    printed = ",".join(str(value) for value in out)
    print(f"interpreter tokens={len(tokens)} nodes={parser.nodes} variables={len(env)} output={printed}")


if __name__ == "__main__":
    main()
