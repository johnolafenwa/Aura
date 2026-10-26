//! Bytecode virtual machine: assemble three stack-machine programs and run them.
//!
//! A two-pass assembler turns text assembly into integer bytecode. A dispatch
//! loop then runs an arithmetic loop, a sieve of Eratosthenes in VM memory, and
//! an iterative Fibonacci sequence modulo a prime.
use std::collections::HashMap;

const ARITHMETIC: &str = "
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
";

const SIEVE: &str = "
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
";

const FIBONACCI: &str = "
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
";

const OPCODES: [&str; 18] = [
    "halt", "push", "load", "store", "add", "sub", "mul", "div", "mod", "lt", "eq", "jmp", "jz",
    "jnz", "loadm", "storem", "out", "dup",
];

fn assemble(text: &str, n: i64) -> Vec<i64> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let mut labels = HashMap::new();
    let mut address = 0;
    for line in &lines {
        if let Some(label) = line.strip_suffix(':') {
            labels.insert(label.to_string(), address);
        } else {
            address += line.split_whitespace().count() as i64;
        }
    }
    let mut code = Vec::new();
    for line in &lines {
        if line.ends_with(':') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        code.push(OPCODES.iter().position(|op| *op == parts[0]).unwrap() as i64);
        if parts.len() == 2 {
            code.push(match parts[1] {
                "N" => n,
                "N1" => n + 1,
                operand => match labels.get(operand) {
                    Some(&address) => address,
                    None => operand.parse().unwrap(),
                },
            });
        }
    }
    code
}

fn execute(code: &[i64], memory_size: usize, out: &mut Vec<i64>) -> i64 {
    let mut stack: Vec<i64> = Vec::new();
    let mut registers = [0i64; 8];
    let mut memory = vec![0i64; memory_size];
    let mut pc = 0usize;
    let mut steps = 0;
    loop {
        let op = code[pc];
        steps += 1;
        match op {
            1 => {
                stack.push(code[pc + 1]);
                pc += 2;
            }
            2 => {
                stack.push(registers[code[pc + 1] as usize]);
                pc += 2;
            }
            3 => {
                registers[code[pc + 1] as usize] = stack.pop().unwrap();
                pc += 2;
            }
            4..=10 => {
                let b = stack.pop().unwrap();
                let a = stack.pop().unwrap();
                stack.push(match op {
                    4 => a + b,
                    5 => a - b,
                    6 => a * b,
                    7 => a / b,
                    8 => a % b,
                    9 => (a < b) as i64,
                    _ => (a == b) as i64,
                });
                pc += 1;
            }
            11 => pc = code[pc + 1] as usize,
            12 => {
                pc = if stack.pop().unwrap() == 0 {
                    code[pc + 1] as usize
                } else {
                    pc + 2
                }
            }
            13 => {
                pc = if stack.pop().unwrap() != 0 {
                    code[pc + 1] as usize
                } else {
                    pc + 2
                }
            }
            14 => {
                let address = stack.pop().unwrap() as usize;
                stack.push(memory[address]);
                pc += 1;
            }
            15 => {
                let address = stack.pop().unwrap() as usize;
                memory[address] = stack.pop().unwrap();
                pc += 1;
            }
            16 => {
                out.push(stack.pop().unwrap());
                pc += 1;
            }
            17 => {
                stack.push(*stack.last().unwrap());
                pc += 1;
            }
            _ => return steps,
        }
    }
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut out = Vec::new();
    let (mut steps, mut code_size) = (0, 0);
    for text in [ARITHMETIC, SIEVE, FIBONACCI] {
        let code = assemble(text, n);
        code_size += code.len();
        steps += execute(&code, (n + 1) as usize, &mut out);
    }
    let printed: Vec<String> = out.iter().map(i64::to_string).collect();
    println!(
        "bytecode_vm n={n} code={code_size} steps={steps} output={}",
        printed.join(",")
    );
}
