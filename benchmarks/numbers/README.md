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
