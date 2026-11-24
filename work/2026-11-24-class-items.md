# Class Items

The class items branch combines ADR-0130 through ADR-0133. Each feature has parse, checker and execution fixtures. The combined fixture exercises a delegated mutable method that writes a property, calls a decorated method, and releases a class with a destructor hook.

Z3 proved all six obligations in the delegated contract example. Changing the argument or field implementation produced a failed precondition or postcondition respectively. The editor snapshot passed 164 language-server tests and 31 VS Code tests. All four language-server coverage measures reached 100%.

The same-host Callgrind baseline at 6642a7c2 used the benchmark inputs from the repository manifest. The Aura counts were 1,331,285,470 instructions for bank_ledger, 870,708,926 for raytracer and 271,220,674 for hashing. The combined compiler and runtime must pass the 0.5% comparison and hosted CI before integration.

Ordinary trait parameter defaults remain deferred. A bound method value would need its trait's defaults to resolve through the same helpers used by a concrete implementation, including after type erasure.
