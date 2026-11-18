# Numbers Benchmarks

Build a release compiler, then run the six workloads from the repository
root.

```sh
cargo build --release -p aura
bash benchmarks/numbers/run.sh
```

The runner builds the Aura and Rust programs before timing and uses Python
3 from the current environment. It prints median milliseconds across three
runs. Set `REPEATS` to change the sample count. Each language times the
work inside a single process. The comparison stops if any checksum changes.

| Case | Work |
| --- | --- |
| Factorial | Compute 100 exact factorials, from 4000! through 4099!, and sum their bit lengths. |
| Fibonacci | Generate 30,000 terms by addition and report the final bit length. |
| Small integers | Sum the integers from 0 through 499,999 in an arbitrary-precision accumulator. |
| Modular powers | Raise each base from 1 through 20,000 to 65,537 modulo 1,000,000,007 and sum the results. |
| Decimal money | Multiply 19.95 by three and add the result to a decimal balance 200,000 times. |
| Mandelbrot | Count iterations across a 320 by 240 grid with a 50-iteration limit. |

Rust uses `num-bigint` at the same version as the Aura runtime,
`rust_decimal`, and `num-complex`. Its factorial uses a balanced product
tree. Python uses its integer and complex builtins, `math.factorial`, and
`decimal.Decimal`. Every decimal operation in this workload is exact.

## Linux x86-64 timings

The release direct backend at commit `cf7d08f5` was measured on one
logical CPU of an Intel Xeon Platinum 8573C on November 17, 2026, using
LLVM 22.1.8, Rust 1.95.0 in release mode and Python 3.12.14. The table
shows median milliseconds from 21 complete runs per language. The three
executables rotated the first position and returned matching checksums.
Compilation and process startup were outside the measured intervals.

| Case | Aura ms | Rust ms | Python ms | Aura / Rust | Checksum |
| --- | ---: | ---: | ---: | ---: | ---: |
| Factorial | 33.267 | 41.011 | 148.487 | 0.81 | 4269323 |
| Fibonacci | 19.876 | 13.606 | 26.139 | 1.46 | 20827 |
| Small integers | 1.109 | 2.198 | 42.084 | 0.50 | 124999750000 |
| Modular powers | 3.611 | 149.414 | 31.589 | 0.02 | 10018095003163 |
| Decimal money | 3.881 | 2.820 | 52.116 | 1.38 | 11970000.00 |
| Mandelbrot | 7.409 | 6.386 | 803.370 | 1.16 | 1551224 |

Aura finished all six workloads faster than Python. Its longest time
relative to Rust was 1.46 times Rust’s time, on Fibonacci.

### Fibonacci carry loop

On November 18, the same Xeon ran a separate paired measurement of the
change in `3d372944`. Each process repeated the full 30,000-term callable
80 times. The four programs rotated their starting position across 11
trials. Every process returned the checksum 1666160. Median process CPU
time per 30,000 terms was 16.754 ms for the previous carry loop, 15.329 ms
for the new loop, 9.156 ms for Rust and 21.164 ms for Python.

The new loop reads both addends independently and writes the result in
groups of 16 words. In 15 full six-workload runs, median Fibonacci wall
time fell from 22.600 ms to 20.358 ms. The six existing checksums matched.
