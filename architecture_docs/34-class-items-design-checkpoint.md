# Class Items Design Checkpoint

This proposal adds trait forwarding, property writes, automatic cleanup, and callable decorators to classes. Each feature preserves the existing receiver modes. An explicit implementation on a class remains the unit of nominal trait conformance.

## Delegation (ADR-0130)

```aura fragment
impl[T: Named] Named for Car[T] by engine
impl Loud for Truck by speaker:
    def volume(self) -> int64:
        return 11
```

Parse contextual `by` after the target of an impl. Accept a single field identifier and an optional ordinary implementation suite. Resolve the trait in the implementation's lexical module and the field in the specialized class. Require an owned field whose type satisfies the exact specialization. Collect missing members across the transitive supertraits, retaining their declaring traits. Reuse an existing coherent parent implementation and deduplicate diamonds. Reject overlapping delegations under the usual coherence rules.

Derive associated bindings from the selected field implementation. An explicit binding must agree with the field for each forwarded signature. Give each explicit override precedence, including an override of a default. For other methods, call the field's selected implementation, including its defaults. Lower a forwarder to an ordinary checked method with a field projection and statically selected call. Shared receivers borrow the projection, mutable receivers reborrow it exclusively, and owning receivers move it. Release any remaining owned fields. For associated methods, select by field type. A trait object built from the class uses its ordinary method table.

Check the substituted trait contract on both explicit and generated methods. Retain parameter names, argument modes, associated-type substitutions, and source spans. Verification of a forwarding body must establish the field callee's preconditions and use its postconditions to establish the wrapper's postconditions. Reject a contract or a signature that cannot hold after substitution, including an unadaptable occurrence of bare `Self` in a result. Avoid treating generated code as a trusted assertion.

## Property Writes (ADR-0131)

```aura fragment
@count.setter
def count(mut self, value: int64):
    self.stored = value
```

Pair a setter with a getter of the same class and name. Require a mutable receiver, one ordinary value parameter of the getter's result type, and a None result. Reject duplicate or mismatched declarations. Preserve the getter's source name and give the setter a separate internal symbol. A property write requires an exclusive mutable receiver place. Resolve assignment to a checked call before normal call and ownership lowering. For augmented assignment, evaluate the receiver place once, read once, evaluate the right operand, apply the ordinary operator, and call the setter once. The read borrow ends before the write. Verify these calls using their actual method contracts. Getter-only writes retain AU2045.

## Automatic Cleanup (ADR-0132)

```aura fragment
def __drop__(mut self):
    self.file.close()
```

Reserve the inherent hook for an exclusive receiver with no parameters, type parameters, or result value. Reject hooks on copy classes, ordinary explicit calls and binding the hook as a callable. Prevent a partial move that would expose an uninitialized field to the hook. A whole-value move transfers the cleanup obligation. Only a fully initialized instance arms its hook.

Derive the need for cleanup from the concrete layout. Types with neither a hook nor owned fields needing a hook use their current lowering. At scope exit, on all control-flow edges, clean live owners in reverse local declaration order. Evaluate the replacement before dropping the old value on assignment. Collection removal transfers ownership if it returns the element. Dropping the removed element, collection, or enclosing class releases its owned values. Disarm each hook before calling it, then release its fields even when the hook traps. Use the existing handle-release machinery and lexical cleanup stack where appropriate. Preserve lexical with-exit semantics. Keep drop bookkeeping out of hook-free code.

A normal cleanup trap propagates after pending cleanup. During an existing unwind, the first trap remains the reported one and pending owners still get cleanup. Apply this rule at a recover boundary and at task termination. Structural Transfer rules apply to classes with hooks. A move across a task boundary transfers the obligation to the receiving owner, including a returned value. Verify the hook as an ordinary mutable method.

## Method Decorators (ADR-0133)

```aura fragment
@logged
def bump(mut self, n: int64):
    self.count += n
```

Accept decorator expressions on inherent and associated methods. Preserve the special forms for getters and setters. Apply the module-level callable rules for defaults, generic decorated signatures, view results, type inference, composition, and initialization order. Include the typed receiver as the first parameter of the unbound callable with its declared mode. Associated methods use their declared parameters. Keep normal member lookup, visibility, and receiver checking before calling the decorated binding, including through a bound method. Recursive calls through the member use the decorated binding. Reuse ordinary module initialization, callable MIR, verifier rules and source locations.

## Questions And Decisions

- Does delegation introduce inheritance? Only the explicit trait and its required parent conformances become available.
- Which implementation supplies a default? Forward to the selected field implementation so its override of a default remains observable.
- Can a delegated owner consume a field of a class with a hook? The same partial-move restriction applies to generated bodies. An explicit consuming override must preserve the hook's valid receiver.
- Can a setter yield a reference to storage? A setter is a write operation. Views, mutable arguments, and borrowed places retain the existing property restrictions.
- Can a cleanup failure mask the original trap? Preserve the first trap and finish releasing live owners.
- Does crossing a task boundary clone a hook? Ownership transfer moves the obligation. A deliberate supported deep clone creates a separate owner.
- Should defaults on ordinary trait method parameters ship here? Include them only if existing call-site default substitution can preserve the trait declaration's defaults through concrete, generic, and dynamic calls without new runtime machinery. Record a separate decision (0134) if implemented.

## Acceptance Evidence

Start with parse, check, diagnostic and execution fixtures for each feature. Exercise native and interpreted output, verification, and editor navigation. Every new diagnostic path needs a library test. Test observable cleanup order, nested collections, moved values, tasks, recovery, and failures inside cleanup. Record same-host Callgrind counts for three unchanged cross-language programs before and after. Use only AU2060 through AU2069, AU3020 through AU3024, and AU4020 through AU4024 for new diagnostics.
