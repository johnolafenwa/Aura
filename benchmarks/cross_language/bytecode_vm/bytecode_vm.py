"""Bytecode virtual machine: assemble three stack-machine programs and run them.

A two-pass assembler turns text assembly into integer bytecode. A dispatch
loop then runs an arithmetic loop, a sieve of Eratosthenes in VM memory, and
an iterative Fibonacci sequence modulo a prime.
"""
import sys

ARITHMETIC = """
    push 0
    store 0
    push 0
    store 1
loop:
    load 0
    push N
    lt
    jz done
    load 1
    load 0
    load 0
    mul
    push 7
    mod
    add
    load 0
    push 3
    mod
    sub
    store 1
    load 0
    push 1
    add
    store 0
    jmp loop
done:
    load 1
    out
    halt
"""

SIEVE = """
    push 2
    store 0
outer:
    load 0
    load 0
    mul
    push N1
    lt
    jz count
    load 0
    loadm
    jnz next
    load 0
    load 0
    mul
    store 1
inner:
    load 1
    push N1
    lt
    jz next
    push 1
    load 1
    storem
    load 1
    load 0
    add
    store 1
    jmp inner
next:
    load 0
    push 1
    add
    store 0
    jmp outer
count:
    push 2
    store 0
    push 0
    store 2
scan:
    load 0
    push N1
    lt
    jz finish
    load 0
    loadm
    jnz skip
    load 2
    push 1
    add
    store 2
skip:
    load 0
    push 1
    add
    store 0
    jmp scan
finish:
    load 2
    out
    halt
"""

FIBONACCI = """
    push 0
    store 0
    push 1
    store 1
    push 0
    store 2
step:
    load 2
    push N
    lt
    jz end
    load 0
    load 1
    dup
    store 0
    add
    push 1000007
    mod
    store 1
    load 2
    push 1
    add
    store 2
    jmp step
end:
    load 0
    out
    halt
"""

OPCODES = {
    "halt": 0, "push": 1, "load": 2, "store": 3, "add": 4, "sub": 5, "mul": 6, "div": 7,
    "mod": 8, "lt": 9, "eq": 10, "jmp": 11, "jz": 12, "jnz": 13, "loadm": 14, "storem": 15,
    "out": 16, "dup": 17,
}


def assemble(text, n):
    lines = []
    for raw in text.splitlines():
        line = raw.strip()
        if line:
            lines.append(line)
    labels = {}
    address = 0
    for line in lines:
        if line.endswith(":"):
            labels[line[:-1]] = address
        else:
            address += len(line.split())
    code = []
    for line in lines:
        if line.endswith(":"):
            continue
        parts = line.split()
        code.append(OPCODES[parts[0]])
        if len(parts) == 2:
            operand = parts[1]
            if operand == "N":
                code.append(n)
            elif operand == "N1":
                code.append(n + 1)
            elif operand in labels:
                code.append(labels[operand])
            else:
                code.append(int(operand))
    return code


def execute(code, memory_size, out):
    stack = []
    registers = [0] * 8
    memory = [0] * memory_size
    pc = 0
    steps = 0
    while True:
        op = code[pc]
        steps += 1
        if op == 1:
            stack.append(code[pc + 1])
            pc += 2
        elif op == 2:
            stack.append(registers[code[pc + 1]])
            pc += 2
        elif op == 3:
            registers[code[pc + 1]] = stack.pop()
            pc += 2
        elif op == 4:
            b = stack.pop()
            stack.append(stack.pop() + b)
            pc += 1
        elif op == 5:
            b = stack.pop()
            stack.append(stack.pop() - b)
            pc += 1
        elif op == 6:
            b = stack.pop()
            stack.append(stack.pop() * b)
            pc += 1
        elif op == 7:
            b = stack.pop()
            stack.append(stack.pop() // b)
            pc += 1
        elif op == 8:
            b = stack.pop()
            stack.append(stack.pop() % b)
            pc += 1
        elif op == 9:
            b = stack.pop()
            stack.append(1 if stack.pop() < b else 0)
            pc += 1
        elif op == 10:
            b = stack.pop()
            stack.append(1 if stack.pop() == b else 0)
            pc += 1
        elif op == 11:
            pc = code[pc + 1]
        elif op == 12:
            pc = code[pc + 1] if stack.pop() == 0 else pc + 2
        elif op == 13:
            pc = code[pc + 1] if stack.pop() != 0 else pc + 2
        elif op == 14:
            stack.append(memory[stack.pop()])
            pc += 1
        elif op == 15:
            address = stack.pop()
            memory[address] = stack.pop()
            pc += 1
        elif op == 16:
            out.append(stack.pop())
            pc += 1
        elif op == 17:
            stack.append(stack[-1])
            pc += 1
        else:
            return steps


def main():
    n = int(sys.argv[1])
    out = []
    steps = 0
    code_size = 0
    for text in (ARITHMETIC, SIEVE, FIBONACCI):
        code = assemble(text, n)
        code_size += len(code)
        steps += execute(code, n + 1, out)
    printed = ",".join(str(value) for value in out)
    print(f"bytecode_vm n={n} code={code_size} steps={steps} output={printed}")


if __name__ == "__main__":
    main()
