# Class Items

The class items branch combines ADR-0130 through ADR-0133. Each feature has parse, checker and execution fixtures. The combined fixture exercises a delegated mutable method that writes a property, calls a decorated method, and releases a class with a destructor hook.

Z3 proved all six obligations in the delegated contract example. Changing the argument or field implementation produced a failed precondition or postcondition respectively. The editor snapshot passed 164 language-server tests and 31 VS Code tests. All four language-server coverage measures reached 100%.

All 31 new run fixtures produced their expected output on the MIR and native back ends. The combined regression covers temporary native values created while a decorated method receives an object with a destructor. The full runnable-fixture layout and native-object suites passed, as did all 1,124 documentation examples and Clippy.

Callgrind measured the release compiler and runtime on the same Linux host using the benchmark manifest inputs. The baseline was 6642a7c2 and the feature revision was 55af91ea.

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| bank_ledger | 1,331,285,470 | 1,330,313,395 | −0.0730% |
| raytracer | 870,708,926 | 870,712,365 | +0.0004% |
| hashing | 271,220,674 | 271,207,983 | −0.0047% |

[Hosted verification and merge status](https://github.com/johnolafenwa/Aura/pull/317)

Ordinary trait parameter defaults remain deferred. A bound method value would need its trait's defaults to resolve through the same helpers used by a concrete implementation, including after type erasure.
