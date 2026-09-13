//! Checker coverage for the `sema/` submodules: alias export validation,
//! callable contract admission, bound and associated method values, returned
//! view projection summaries through methods and traits, union injection
//! through grouped and branching arguments, pattern coverage rows, and the
//! clone-safety and task-transfer property walks. Each test pins one branch
//! the fixture suites did not reach.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aura_compiler::{
    check_path, check_source, emit_host_native_object, lower_source_to_mir, run_mir, run_path,
    run_source, Span,
};

fn rejects(source: &str, expected: &str) -> aura_compiler::Diagnostic {
    let error = check_source(source).expect_err("source should be rejected");
    assert!(
        error.message.contains(expected),
        "diagnostic `{}` should mention `{expected}`",
        error.message
    );
    error
}

fn accepts(source: &str) {
    check_source(source).unwrap_or_else(|error| panic!("source should check: {}", error.message));
}

fn runs(source: &str, expected_stdout: &str) {
    let output =
        run_source(source).unwrap_or_else(|error| panic!("source should run: {}", error.message));
    assert_eq!(output.stdout, expected_stdout);
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let unique = format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("failed to create temp dir");
        Self { path }
    }

    fn write(&self, relative: &str, source: &str) -> PathBuf {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dirs");
        }
        fs::write(&path, source).expect("failed to write module source");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

const PRIVATE_BOX_API: &str = r#"public class Box:
    public tag: str

    def make(value: int64) -> Box:
        return Box(tag="m")

    def bump(self) -> int64:
        return 1
"#;

// ---------------------------------------------------------------------------
// sema/aliases.rs
// ---------------------------------------------------------------------------

#[test]
fn public_alias_of_a_private_enum_is_rejected() {
    rejects(
        "enum Shape:\n    Circle\npublic type S = Shape\ndef main():\n    pass\n",
        "public type alias `S` exposes private type `Shape`",
    );
}

#[test]
fn public_alias_of_a_private_opaque_handle_is_rejected() {
    let temp = TempDir::new("aura-sema-opaque-alias");
    temp.write(
        "Aura.toml",
        "[package]\nname = \"opaque_alias\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
    );
    let main_path = temp.write(
        "src/main.au",
        "extern \"C\" opaque class Handle\npublic type H = Handle\ndef main():\n    pass\n",
    );
    let error = check_path(&main_path).expect_err("a private opaque handle must not be exported");
    assert!(
        error
            .message
            .contains("public type alias `H` exposes private type `Handle`"),
        "{}",
        error.message
    );
}

#[test]
fn public_alias_exposing_a_private_returned_view_pointee_is_rejected() {
    rejects(
        "class Secret:\n    value: int64\npublic class Holder:\n    inner: Secret\npublic type Peek = def(h: Holder) -> view Secret from h\ndef main():\n    pass\n",
        "public type alias `Peek` exposes private type `Secret`",
    );
}

#[test]
fn public_callable_alias_names_the_private_type_inside_its_signature() {
    let error = rejects(
        "class Secret:\n    value: int64\npublic type Cb = Callable[def(s: Secret) -> None]\ndef main():\n    pass\n",
        "public type alias `Cb` exposes private type `Secret`",
    );
    assert_eq!(error.span, Some(Span::new(3, 34)));
}

#[test]
fn grouped_indexed_alias_callee_expands_to_the_specialized_constructor() {
    // The checker expands the grouped, indexed alias callee and lowering
    // looks through the same group, so the program runs.
    runs(
        "class Box[T]:\n    value: T\ntype Wrapped[T] = Box[T]\ndef main():\n    w = (Wrapped[int64])(value=1)\n    print(w.value)\n",
        "1\n",
    );
}

#[test]
fn indexed_alias_callee_requires_type_arguments() {
    rejects(
        "class Box[T]:\n    value: T\ntype Wrapped[T] = Box[T]\ndef main():\n    w = Wrapped[1](value=1)\n    print(w.value)\n",
        "type alias specialization expects type arguments",
    );
}

#[test]
fn specialized_associated_method_through_an_alias_expands_the_alias_object() {
    runs(
        "class Box:\n    tag: str\n    def make[T](value: own T) -> Box:\n        return Box(tag=\"m\")\ntype Wrapped = Box\ndef main():\n    w = Wrapped.make[int64](1)\n    print(w.tag)\n",
        "m\n",
    );
}

const BOUNDED_ALIAS: &str = "trait Show:\n    def show(self) -> str\nclass Box[T]:\n    value: T\ntype Boxed[T: Show] = Box[T]\n";

#[test]
fn bounded_alias_in_a_signature_accepts_a_satisfying_argument() {
    runs(
        &format!(
            "{BOUNDED_ALIAS}class H:\n    x: int64\nimpl Show for H:\n    def show(self) -> str:\n        return \"h\"\ndef render(b: Boxed[H]) -> str:\n    return b.value.show()\ndef main():\n    print(render(Box(value=H(x=1))))\n"
        ),
        "h\n",
    );
}

#[test]
fn bounded_alias_violations_surface_from_every_declaration_kind() {
    let expected = "type `int64` does not implement trait `Show`";
    rejects(
        &format!("{BOUNDED_ALIAS}public class H:\n    x: int64\n    public def m(self, b: Boxed[int64]) -> int64:\n        return 1\ndef main():\n    pass\n"),
        expected,
    );
    rejects(
        &format!("{BOUNDED_ALIAS}public trait Taker:\n    def m(self, b: Boxed[int64]) -> int64\ndef main():\n    pass\n"),
        expected,
    );
    rejects(
        &format!("{BOUNDED_ALIAS}impl Show for Boxed[int64]:\n    def show(self) -> str:\n        return \"x\"\ndef main():\n    pass\n"),
        expected,
    );
    rejects(
        &format!("{BOUNDED_ALIAS}class H:\n    x: int64\nimpl Show for H:\n    def show(self) -> str:\n        return \"h\"\ntrait Taker:\n    def take(self, b: Boxed[int64]) -> int64\nimpl Taker for H:\n    def take(self, b: Boxed[int64]) -> int64:\n        return 1\ndef main():\n    pass\n"),
        expected,
    );
}

// ---------------------------------------------------------------------------
// sema/callables.rs
// ---------------------------------------------------------------------------

#[test]
fn consuming_closure_cannot_be_packed_as_a_shared_callable() {
    rejects(
        "type Shared = Callable[def() -> int64]\nclass Holder:\n    x: int64\ndef consume(h: own Holder) -> int64:\n    return h.x\ndef main():\n    holder = Holder(x=1)\n    packed = Shared(lambda [own holder]: consume(holder))\n    print(packed())\n",
        "a Consuming value cannot be packed as Shared `Shared`",
    );
}

#[test]
fn contract_admission_reports_positional_only_slots_made_keyword_only() {
    rejects(
        "def twice(value: int64) -> int64:\n    return value * 2\ndef main():\n    g: def(int64) -> int64 = twice\n    h: def(*, value: int64) -> int64 = g\n    print(h(value=1))\n",
        "positional parameter 1 is positional-only, but the destination makes it keyword-only `value`",
    );
}

#[test]
fn contract_admission_reports_keyword_only_renames() {
    rejects(
        "def twice(value: int64) -> int64:\n    return value * 2\ndef main():\n    h: def(*, other: int64) -> int64 = twice\n    print(h(other=1))\n",
        "keyword-only parameter `value` would be renamed to `other`",
    );
}

#[test]
fn contract_admission_reports_unnamed_slots_given_a_name() {
    rejects(
        "def twice(value: int64) -> int64:\n    return value * 2\ndef main():\n    g: def(int64) -> int64 = twice\n    h: def(value: int64) -> int64 = g\n    print(h(1))\n",
        "positional parameter 1 has no exposed name, but the destination names it `value`",
    );
}

#[test]
fn conditional_branches_cannot_merge_packed_callables_with_different_slot_names() {
    rejects(
        "type A = Callable[def(value: int64) -> int64]\ntype B = Callable[def(other: int64) -> int64]\ndef main():\n    a = A(lambda value: value)\n    b = B(lambda other: other)\n    flag = true\n    c = a if flag else b\n    print(1)\n",
        "parameter `value` differs from parameter `other`",
    );
}

#[test]
fn collection_callbacks_cannot_return_views() {
    rejects(
        "class Pair:\n    left: str\ndef pick(pair: Pair) -> view str from pair:\n    return view pair.left\ndef main():\n    pairs = [Pair(left=\"a\")]\n    print(pairs.map(pick).len())\n",
        "`list.map` callback cannot return a view",
    );
}

#[test]
fn lambda_capture_scan_walks_is_none_tests_and_format_literals() {
    runs(
        "def main():\n    value: int64 | None = 1\n    check = lambda [value]: value is None\n    print(check())\n",
        "false\n",
    );
    runs(
        "def main():\n    name = \"aura\"\n    greet = lambda [name]: f\"hello {name}\"\n    print(greet())\n",
        "hello aura\n",
    );
}

#[test]
fn lambda_keyword_only_parameter_cannot_fill_a_positional_slot() {
    rejects(
        "def main():\n    f: def(value: int64) -> int64 = lambda *, value: value\n    print(f(1))\n",
        "but the expected contract keeps it positional",
    );
}

#[test]
fn specialized_member_and_grouped_callees_are_not_bare_names() {
    runs(
        "class Box:\n    tag: str\n    def make[T](value: own T) -> Box:\n        return Box(tag=\"m\")\ndef main():\n    b = Box.make[int64](1)\n    print(b.tag)\n",
        "m\n",
    );
    runs(
        "def twice(v: int64) -> int64:\n    return v * 2\ndef main():\n    print((twice)(1))\n",
        "2\n",
    );
}

#[test]
fn bare_variant_constructor_without_an_expected_type_is_rejected() {
    rejects(
        "def main():\n    x = Some(1)\n",
        "bare enum variants require an expected enum type",
    );
}

#[test]
fn task_targets_accept_grouped_names_and_grouped_type_arguments() {
    runs(
        "def make() -> int64:\n    return 1\ndef main():\n    with TaskGroup() as group:\n        task = group.start((make))\n        print(task.result_or(0, timeout=30s))\n",
        "1\n",
    );
    runs(
        "def make[T](v: own T) -> int64:\n    return 1\nclass Box:\n    x: int64\ndef main():\n    with TaskGroup() as group:\n        task = group.start(make[(Box)], Box(x=1))\n        print(task.result_or(0, timeout=30s))\n",
        "1\n",
    );
}

#[test]
fn task_target_associated_method_type_argument_count_is_checked() {
    rejects(
        "class Box:\n    x: int64\n    def make[T](v: own T) -> int64:\n        return 1\ndef main():\n    with TaskGroup() as group:\n        task = group.start(Box.make[int64, str], 1)\n",
        "associated method `Box.make` expects 1 type argument, found 2",
    );
}

#[test]
fn private_methods_of_imported_classes_are_not_values() {
    let temp = TempDir::new("aura-sema-private-methods");
    temp.write("api.au", PRIVATE_BOX_API);
    let associated = temp.write(
        "associated.au",
        "import api\n\ndef main():\n    f = api.Box.make\n    print(1)\n",
    );
    let error = check_path(&associated).expect_err("a private associated method is not a value");
    assert!(
        error.message.contains("method `make` is private on `Box`"),
        "{}",
        error.message
    );

    let bound = temp.write(
        "bound.au",
        "import api\n\ndef main():\n    box = api.Box(tag=\"a\")\n    f = box.bump\n    print(1)\n",
    );
    let error = check_path(&bound).expect_err("a private receiver method cannot be bound");
    assert!(
        error.message.contains("method `bump` is private on `Box`"),
        "{}",
        error.message
    );
}

#[test]
fn associated_method_accessed_through_an_instance_is_not_a_field() {
    rejects(
        "class Box:\n    tag: str\n    def make(value: int64) -> Box:\n        return Box(tag=\"m\")\ndef main():\n    box = Box(tag=\"a\")\n    f = box.make\n",
        "method `make` on `Box` cannot be used as a field",
    );
}

#[test]
fn indexed_member_on_a_tuple_receiver_is_not_a_method_specialization() {
    rejects(
        "def main():\n    t = (1, 2)\n    print(t.first[0])\n",
        "cannot access field `first` on `(int64, int64)`",
    );
}

#[test]
fn trait_receiver_methods_accept_explicit_type_arguments_after_the_member() {
    runs(
        "trait Conv:\n    def conv[T](self, value: own T) -> T\nclass Box:\n    tag: str\nimpl Conv for Box:\n    def conv[T](self, value: own T) -> T:\n        return value\ndef main():\n    box = Box(tag=\"a\")\n    print(box.conv[int64](1))\n",
        "1\n",
    );
}

#[test]
fn bound_method_explicit_type_argument_count_uses_the_plural_form() {
    rejects(
        "class Box:\n    tag: str\n    def conv[A, B](self, a: own A, b: own B) -> int64:\n        return 1\ndef main():\n    box = Box(tag=\"a\")\n    f = box.conv[int64]\n",
        "method `Box.conv` expects 2 type arguments, found 1",
    );
}

#[test]
fn bound_method_inference_reads_an_expected_closure_contract() {
    rejects(
        "class Box:\n    tag: str\n    def convert[T](self, value: own T) -> T:\n        return value\ndef main():\n    box = Box(tag=\"a\")\n    tag = \"x\"\n    mut f = lambda [tag]: tag.len()\n    f = box.convert\n",
        "cannot specialize method `Box.convert`: expected 1 parameter, found 0",
    );
}

const GENERIC_METHOD_BOX: &str = "class Box:\n    tag: str\n    def convert[T](self, value: own T) -> T:\n        return value\n    def wrap[T](self, value: list[T]) -> int64:\n        return 1\n    def make[T](self) -> int64:\n        return 1\n";

#[test]
fn bound_method_inference_reports_each_contract_mismatch() {
    rejects(
        &format!("{GENERIC_METHOD_BOX}def main():\n    box = Box(tag=\"a\")\n    f: def(int64, int64) -> int64 = box.convert\n"),
        "cannot specialize method `Box.convert`: expected 1 parameter, found 2",
    );
    rejects(
        &format!("{GENERIC_METHOD_BOX}def main():\n    box = Box(tag=\"a\")\n    f: def(str) -> int64 = box.convert\n"),
        "cannot specialize method `Box.convert`: conflicting inferred types for `T`: `str` and `int64`",
    );
    rejects(
        &format!("{GENERIC_METHOD_BOX}def main():\n    box = Box(tag=\"a\")\n    f: def(int64) -> int64 = box.wrap\n"),
        "cannot specialize method `Box.wrap`: expected `list[T]`, found `int64`",
    );
    rejects(
        &format!("{GENERIC_METHOD_BOX}def main():\n    box = Box(tag=\"a\")\n    f: def() -> int64 = box.make\n"),
        "cannot infer type parameter `T` for method `Box.make`",
    );
}

const CONSUMING_COUNTER: &str = "class Counter:\n    total: int64\n    def finish(own self) -> int64:\n        return self.total\n";

#[test]
fn consuming_bound_method_takes_a_grouped_owned_receiver() {
    runs(
        &format!("{CONSUMING_COUNTER}def main():\n    counter = Counter(total=1)\n    done = (counter).finish\n    print(done())\n"),
        "1\n",
    );
}

#[test]
fn consuming_bound_method_rejects_view_and_borrowed_receivers() {
    let error = rejects(
        &format!("{CONSUMING_COUNTER}def main():\n    counter = Counter(total=1)\n    view alias = counter\n    done = alias.finish\n    print(done())\n"),
        "bound method `Counter.finish` cannot take ownership of the pointee of view `alias`",
    );
    assert_eq!(error.code, "AU3004");
    rejects(
        &format!("{CONSUMING_COUNTER}def bind(counter: Counter) -> int64:\n    done = counter.finish\n    return done()\ndef main():\n    pass\n"),
        "cannot take shared parameter `counter` by value",
    );
    rejects(
        &format!("{CONSUMING_COUNTER}def bind(counter: mut Counter) -> int64:\n    done = counter.finish\n    return done()\ndef main():\n    pass\n"),
        "cannot take mutable value `counter` by value",
    );
    rejects(
        &format!("{CONSUMING_COUNTER}def main():\n    value: Counter | None = Counter(total=1)\n    match value:\n        case Counter as c:\n            done = c.finish\n            print(done())\n        case None:\n            pass\n"),
        "cannot take shared value `c` by value",
    );
}

#[test]
fn function_type_capability_mismatch_names_the_mut_capability() {
    rejects(
        "class Counter:\n    total: int64\ndef takes_shared(c: Counter):\n    pass\ndef main():\n    f: def(mut Counter) -> None = takes_shared\n",
        "has `shared` capability, but `def(mut Counter) -> None` requires `mut`",
    );
}

#[test]
fn unresolved_union_member_parameter_reports_the_union_member_rule() {
    rejects(
        "def pick[T](value: T | int64) -> int64:\n    return 1\ndef main():\n    x: int64 = 1\n    print(pick(x))\n",
        "cannot infer type parameter `T` for function `pick` from its union member arguments",
    );
}

// ---------------------------------------------------------------------------
// sema/loans.rs
// ---------------------------------------------------------------------------

#[test]
fn view_returning_callable_values_keep_owned_parameter_modes() {
    runs(
        "class Pair:\n    left: str\ndef pick(tag: own int64, pair: Pair) -> view str from pair:\n    return view pair.left\ndef main():\n    pair = Pair(left=\"a\")\n    chooser: def(tag: own int64, pair: Pair) -> view str from pair = pick\n    view head = chooser(1, pair)\n    print(head)\n",
        "a\n",
    );
}

const PAIR_CLASS: &str = "class Pair:\n    left: int64\n    right: int64\n";

#[test]
fn nested_view_through_an_inherent_method_keeps_the_sibling_field_unlocked() {
    runs(
        &format!("{PAIR_CLASS}class Holder:\n    pair: Pair\n    other: int64\n    def inner(self) -> view Pair from self:\n        return view self.pair\n    def left_of(self) -> view int64 from self:\n        return view self.inner().left\ndef main():\n    mut holder = Holder(pair=Pair(left=1, right=2), other=3)\n    view l = holder.left_of()\n    holder.other = 4\n    print(l)\n"),
        "1\n",
    );
}

#[test]
fn nested_view_through_a_trait_impl_method_keeps_the_sibling_field_unlocked() {
    runs(
        &format!("{PAIR_CLASS}trait Viewer:\n    def get(self) -> view Pair from self\nclass Holder:\n    pair: Pair\n    other: int64\n    def left_of(self) -> view int64 from self:\n        return view self.get().left\nimpl Viewer for Holder:\n    def get(self) -> view Pair from self:\n        return view self.pair\ndef main():\n    mut holder = Holder(pair=Pair(left=1, right=2), other=3)\n    view l = holder.left_of()\n    holder.other = 4\n    print(l)\n"),
        "1\n",
    );
}

#[test]
fn nested_view_inside_a_trait_default_method_resolves_the_abstract_callee() {
    runs(
        &format!("{PAIR_CLASS}trait Viewer:\n    def get(self) -> view Pair from self\n    def left_of(self) -> view int64 from self:\n        return view self.get().left\nclass Holder:\n    pair: Pair\n    other: int64\nimpl Viewer for Holder:\n    def get(self) -> view Pair from self:\n        return view self.pair\ndef main():\n    mut holder = Holder(pair=Pair(left=1, right=2), other=3)\n    view l = holder.left_of()\n    holder.other = 4\n    print(l)\n"),
        "1\n",
    );
    runs(
        &format!("{PAIR_CLASS}trait Viewer:\n    def get(self) -> view Pair from self\n    def left_of(self, flag: bool) -> view int64 from self:\n        return view self.get().left\nclass Holder:\n    pair: Pair\n    other: int64\nimpl Viewer for Holder:\n    def get(self) -> view Pair from self:\n        return view self.pair\ndef main():\n    mut holder = Holder(pair=Pair(left=1, right=2), other=3)\n    view l = holder.left_of(true)\n    holder.other = 4\n    print(l)\n"),
        "1\n",
    );
}

#[test]
fn view_projection_through_a_tuple_element_is_typed_by_position() {
    runs(
        &format!("{PAIR_CLASS}class Holder:\n    pair: (Pair, int64)\n    other: int64\n    def first_left(self) -> view int64 from self:\n        return view self.pair[0].left\ndef main():\n    mut holder = Holder(pair=(Pair(left=1, right=2), 5), other=3)\n    view l = holder.first_left()\n    holder.other = 4\n    print(l)\n"),
        "1\n",
    );
}

#[test]
fn dynamic_or_negative_index_projections_keep_a_conservative_footprint() {
    let dynamic = rejects(
        "class Holder:\n    items: list[int64]\n    other: int64\ndef pick(holder: Holder, index: int64) -> view int64 from holder:\n    return view holder.items[index]\ndef main():\n    mut holder = Holder(items=[1, 2], other=0)\n    view chosen = pick(holder, 0)\n    holder.other = 3\n    print(chosen)\n",
        "cannot mutate `holder.other` while shared view `chosen` remains live",
    );
    assert_eq!(dynamic.code, "AU3002");
    rejects(
        "class Holder:\n    items: (int64, int64)\n    other: int64\ndef pick(holder: Holder) -> view int64 from holder:\n    return view holder.items[-1]\ndef main():\n    mut holder = Holder(items=(1, 2), other=0)\n    view chosen = pick(holder)\n    holder.other = 3\n    print(chosen)\n",
        "cannot mutate `holder.other` while shared view `chosen` remains live",
    );
}

#[test]
fn forwarding_a_whole_origin_through_a_projected_argument_keeps_the_projection() {
    runs(
        &format!("{PAIR_CLASS}class Holder:\n    pair: Pair\n    other: int64\ndef whole(pair: Pair) -> view Pair from pair:\n    return view pair\ndef outer(holder: Holder) -> view Pair from holder:\n    return view whole(holder.pair)\ndef main():\n    mut holder = Holder(pair=Pair(left=1, right=2), other=0)\n    view chosen = outer(holder)\n    holder.other = 3\n    print(chosen.left)\n"),
        "1\n",
    );
}

#[test]
fn forwarded_view_calls_accept_explicit_type_arguments_and_defaulted_slots() {
    runs(
        &format!("{PAIR_CLASS}def inner[T](pair: Pair, tag: own T) -> view int64 from pair:\n    return view pair.left\ndef outer(pair: Pair) -> view int64 from pair:\n    return view inner[int64](pair, 1)\ndef main():\n    mut pair = Pair(left=1, right=2)\n    view chosen = outer(pair)\n    pair.right = 3\n    print(chosen)\n"),
        "1\n",
    );
    runs(
        &format!("{PAIR_CLASS}def inner(flag: bool, pair: Pair) -> view int64 from pair:\n    return view pair.left\ndef outer(pair: Pair) -> view int64 from pair:\n    return view inner(true, pair)\ndef main():\n    mut pair = Pair(left=1, right=2)\n    view chosen = outer(pair)\n    pair.right = 3\n    print(chosen)\n"),
        "1\n",
    );
}

#[test]
fn self_recursive_trait_view_impl_checks_but_both_backends_reject_its_lowered_loan() {
    // Documented limit, not end-to-end success coverage. `Loop.get` forwards
    // to itself, which the checker's summary records as a deferred cycle
    // rather than an unknown projection, so the program checks and the
    // `Left` footprint still narrows to `pair.left`. The lowered module then
    // fails the shared validator: the recursive impl's returned loan projects
    // `self` with the whole `Loop` type where an `int64` view is declared.
    // Both public boundaries refuse it for that one reason, so the program
    // never runs on either backend.
    let source = format!("{PAIR_CLASS}trait Viewer:\n    def get(self) -> view int64 from self\nclass Left:\n    pair: Pair\nclass Loop:\n    pair: Pair\nimpl Viewer for Left:\n    def get(self) -> view int64 from self:\n        return view self.pair.left\nimpl Viewer for Loop:\n    def get(self) -> view int64 from self:\n        return view self.get()\ndef forward[T: Viewer](value: T) -> view int64 from value:\n    return view value.get()\ndef main():\n    mut left = Left(pair=Pair(left=1, right=2))\n    view chosen = forward(left)\n    left.pair.right = 3\n    print(chosen)\n");
    accepts(&source);
    let mir = lower_source_to_mir(&source).expect("the checked program lowers");
    let interpreted = run_mir(&mir).expect_err("the interpreter refuses the recursive impl's loan");
    let native = emit_host_native_object(&mir)
        .expect_err("the direct backend refuses the recursive impl's loan");
    assert_eq!(
        interpreted.message.strip_prefix("invalid MIR loan flow: "),
        Some(native.as_str()),
        "both boundaries must report the same shared validator reason"
    );
    assert!(
        native.contains("invalid MIR loan `%t0` in `Viewer for Loop.get` projects `self`"),
        "{native}"
    );
}

#[test]
fn view_through_an_imported_associated_method_resolves_the_module_class() {
    let temp = TempDir::new("aura-sema-imported-view-class");
    temp.write(
        "api.au",
        "public class LeftBox:\n    public left: int64\n    public right: int64\n\n    public def associated(value: LeftBox) -> view int64 from value:\n        return view value.left\n",
    );
    let main_path = temp.write(
        "main.au",
        "import api\n\nclass Holder:\n    box: api.LeftBox\n    other: int64\n\n    def left(self) -> view int64 from self:\n        return view api.LeftBox.associated(self.box)\n\ndef main():\n    mut holder = Holder(box=api.LeftBox(left=1, right=2), other=0)\n    view chosen = holder.left()\n    holder.other = 3\n    holder.box.right = 4\n    print(chosen)\n",
    );
    // The checker resolves `api.LeftBox` through the module namespace and
    // narrows the footprint to `box.left`; lowering resolves the same
    // associated callee, so the sibling writes stay legal at run time.
    check_path(&main_path).expect("the projected imported view leaves siblings unlocked");
    let output = run_path(&main_path).expect("the imported associated view forward runs");
    assert_eq!(output.stdout, "1\n");
}

#[test]
fn task_targets_inside_an_imported_module_resolve_through_its_namespace() {
    let temp = TempDir::new("aura-sema-imported-task-target");
    temp.write(
        "api.au",
        "def helper() -> int64:\n    return 7\n\npublic def run() -> int64:\n    with TaskGroup() as group:\n        task = group.start(helper)\n        return task.result_or(0, timeout=30s)\n",
    );
    let main_path = temp.write(
        "main.au",
        "import api\n\ndef main():\n    print(api.run())\n",
    );
    let output = run_path(&main_path).expect("the imported task target runs");
    assert_eq!(output.stdout, "7\n");
}

#[test]
fn unknown_qualified_module_members_fall_through_every_namespace_lookup() {
    let temp = TempDir::new("aura-sema-unknown-module-member");
    temp.write("api.au", PRIVATE_BOX_API);
    let main_path = temp.write(
        "main.au",
        "import api\n\ndef main():\n    x = api.Missing(1)\n",
    );
    let error = check_path(&main_path).expect_err("an unknown module member is rejected");
    assert!(
        error
            .message
            .contains("module `api` has no callable member `Missing`"),
        "{}",
        error.message
    );
}

#[test]
fn mutable_returned_view_nested_under_a_shared_slot_is_rejected() {
    let error = rejects(
        "class Pair:\n    left: int64\ndef wrap(pair: mut Pair) -> view mut Pair from pair:\n    return view mut pair\ndef pick(pair: Pair) -> view int64 from pair:\n    return view pair.left\ndef show(value: int64) -> int64:\n    return value\ndef main():\n    mut pair = Pair(left=1)\n    print(show(pick(wrap(pair))))\n",
        "a mutable returned view requires a mutable view binding or immediate mutable reborrow",
    );
    assert_eq!(error.span, Some(Span::new(11, 21)));
}

#[test]
fn match_mut_accepts_a_grouped_place_and_rejects_a_call_scrutinee() {
    accepts(
        "class Pair:\n    left: int64\ndef main():\n    mut value: Pair | None = Pair(left=1)\n    match mut (value):\n        case Pair as p:\n            p.left = 2\n        case None:\n            pass\n",
    );
    rejects(
        "class Pair:\n    left: int64\ndef make() -> Pair | None:\n    return Pair(left=1)\ndef main():\n    match mut make():\n        case Pair as p:\n            p.left = 2\n        case None:\n            pass\n",
        "`match mut` requires a mutable place scrutinee",
    );
}

// ---------------------------------------------------------------------------
// sema/unions.rs and sema/capabilities.rs
// ---------------------------------------------------------------------------

const SHOW_UNION: &str = "def show(value: int64 | str):\n    print(value)\n";

#[test]
fn borrowed_union_injection_looks_through_groups_conditionals_and_matches() {
    let expected = "borrowed union argument cannot implicitly clone member place of type 'str'";
    rejects(
        &format!(
            "{SHOW_UNION}def main():\n    text = \"aura\"\n    show((text))\n    print(text)\n"
        ),
        expected,
    );
    rejects(
        &format!("{SHOW_UNION}def main():\n    text = \"aura\"\n    other = \"beta\"\n    flag = true\n    show(text if flag else other)\n    print(text)\n"),
        expected,
    );
    rejects(
        &format!("{SHOW_UNION}def main():\n    text = \"aura\"\n    flag = true\n    show(match flag:\n        case true: text\n        case false: 1)\n    print(text)\n"),
        expected,
    );
}

#[test]
fn union_literal_probe_passes_through_non_type_errors() {
    let error = rejects(
        "def main():\n    value: list[int64] | str = [missing]\n    print(1)\n",
        "unknown name `missing`",
    );
    assert_eq!(error.code, "AU2001");
}

#[test]
fn reading_a_narrowed_copy_member_does_not_consume_the_union() {
    runs(
        "def main():\n    value: int64 | None = 7\n    if value is not None:\n        copy = value\n        print(copy)\n    print(value is None)\n",
        "7\nfalse\n",
    );
}

#[test]
fn moving_a_grouped_borrowed_parameter_names_the_root_binding() {
    rejects(
        "class Holder:\n    x: int64\ndef consume(h: own Holder):\n    pass\ndef f(h: Holder):\n    consume((h))\ndef main():\n    pass\n",
        "parameter `h` is borrowed; declare it as `own Holder`",
    );
}

#[test]
fn moving_a_non_cloneable_shared_match_payload_says_it_cannot_be_cloned() {
    let error = rejects(
        "type Cb = Callable[def() -> int64]\ndef consume(cb: own Cb) -> int64:\n    return cb()\ndef main():\n    value: Cb | None = Cb(lambda: 1)\n    match value:\n        case Cb as cb:\n            print(consume(cb))\n        case None:\n            print(0)\n",
        "cannot move `cb` out of a shared match on `value`",
    );
    assert!(
        error
            .help
            .iter()
            .any(|help| help.contains("`Callable[def() -> int64]` cannot be cloned")),
        "{:?}",
        error.help
    );
}

// ---------------------------------------------------------------------------
// sema/types.rs
// ---------------------------------------------------------------------------

#[test]
fn alias_constructor_type_arguments_spell_callable_and_view_returning_members() {
    runs(
        "type Cb = Callable[def() -> int64]\nclass Box[T]:\n    value: T\ntype Wrapped = Box[Cb]\ndef main():\n    w = Wrapped(value=Cb(lambda: 1))\n    print(w.value())\n",
        "1\n",
    );
    runs(
        "class Pair:\n    left: str\ndef pick(pair: Pair) -> view str from pair:\n    return view pair.left\ntype Picker = def(pair: Pair) -> view str from pair\nclass Box[T]:\n    value: T\ntype Wrapped = Box[Picker]\ndef main():\n    w = Wrapped(value=pick)\n    pair = Pair(left=\"a\")\n    view head = w.value(pair)\n    print(head)\n",
        "a\n",
    );
}

#[test]
fn union_member_lowering_passes_through_unknown_type_errors() {
    let error = rejects(
        "def f(v: int64 | Missing):\n    pass\ndef main():\n    pass\n",
        "unknown type `Missing`",
    );
    assert_eq!(error.code, "AU2001");
}

#[test]
fn callable_pattern_unification_rejects_other_shapes_and_capabilities() {
    rejects(
        "def run[T](f: Callable[def(own T) -> T], v: own T) -> T:\n    return f(v)\ndef main():\n    print(run(1, 2))\n",
        "expected `Callable[def(own T) -> T]`, found `int64`",
    );
    rejects(
        "type Shared = Callable[def(int64) -> int64]\ndef run[T](f: Callable[def(own T) -> T], v: own T) -> T:\n    return f(v)\ndef main():\n    cb = Shared(lambda x: x)\n    print(run(cb, 2))\n",
        "expected `Callable[def(own T) -> T]`, found `Callable[def(int64) -> int64]`",
    );
}

#[test]
fn union_result_pattern_with_no_remainder_asks_for_explicit_specialization() {
    rejects(
        "def pick[T]() -> T | int64:\n    return 1\ndef main():\n    x: int64 = pick()\n    print(x)\n",
        "every member is already named by `int64 | T`, so specialize the callable explicitly",
    );
}

#[test]
fn function_type_view_mut_result_requires_a_mut_origin_slot() {
    rejects(
        "class Pair:\n    left: int64\ndef pick(pair: mut Pair) -> view mut int64 from pair:\n    return view mut pair.left\ndef main():\n    f: def(pair: Pair) -> view mut int64 from pair = pick\n",
        "a `view mut` result requires its origin parameter `pair` to be `mut`",
    );
}

#[test]
fn imported_alias_type_argument_count_is_checked() {
    let temp = TempDir::new("aura-sema-imported-alias-arity");
    temp.write("api.au", "public type Boxes[T] = list[T]\n");
    let main_path = temp.write(
        "main.au",
        "import api\n\ndef main():\n    items: api.Boxes[int64, str] = []\n    print(items.len())\n",
    );
    let error = check_path(&main_path).expect_err("an imported alias keeps its arity");
    assert!(
        error
            .message
            .contains("`api.Boxes` expects exactly 1 type arguments, found 2"),
        "{}",
        error.message
    );
}

#[test]
fn local_alias_type_argument_count_uses_the_plural_form() {
    rejects(
        "type Pair2[A, B] = (A, B)\ndef main():\n    x: Pair2[int64] = (1, 2)\n",
        "`Pair2` expects exactly 2 type arguments, found 1",
    );
}

#[test]
fn generic_signatures_collect_type_parameters_from_callables_and_unions() {
    accepts(
        "def apply[T](f: Callable[def(own T) -> T], v: own T) -> T:\n    return f(v)\ndef main():\n    pass\n",
    );
    runs(
        "def f[T](value: T | None) -> int64:\n    return 1\ndef main():\n    print(f[int64](1))\n",
        "1\n",
    );
}

// ---------------------------------------------------------------------------
// sema/patterns.rs
// ---------------------------------------------------------------------------

#[test]
fn or_patterns_over_a_union_scrutinee_contribute_their_typed_members() {
    runs(
        "def main():\n    value: int64 | str | None = 1\n    match value:\n        case int64 as n:\n            print(n)\n        case str as s:\n            print(s)\n        case None | None:\n            print(\"none\")\n",
        "1\n",
    );
}

#[test]
fn guarded_wildcards_are_not_final_catch_alls() {
    runs(
        "enum Shape:\n    Circle\n    Square\ndef main():\n    shape = Shape.Circle\n    flag = true\n    match shape:\n        case _ if flag:\n            print(1)\n        case _:\n            print(2)\n",
        "1\n",
    );
    runs(
        "def main():\n    value = 1\n    flag = true\n    x = match value:\n        case _ if flag: 1\n        case _: 2\n    print(x)\n",
        "1\n",
    );
}

#[test]
fn literal_match_expressions_over_integers_track_non_bool_literals() {
    runs(
        "def main():\n    value = 1\n    x = match value:\n        case 1: \"a\"\n        case _: \"b\"\n    print(x)\n",
        "a\n",
    );
}

#[test]
fn or_pattern_alternatives_must_match_the_scrutinee_type() {
    rejects(
        "def main():\n    value = 1\n    match value:\n        case 1 | \"a\":\n            print(1)\n        case _:\n            print(2)\n",
        "literal pattern \"a\" does not match scrutinee type `int64`",
    );
}

#[test]
fn duplicate_float_and_bool_literal_arms_are_rendered() {
    rejects(
        "def main():\n    value = 1.5\n    match value:\n        case 1.5:\n            print(1)\n        case 1.5:\n            print(2)\n        case _:\n            print(3)\n",
        "duplicate match arm for literal `1.5`",
    );
    rejects(
        "def main():\n    value = true\n    match value:\n        case true:\n            print(1)\n        case true:\n            print(2)\n        case false:\n            print(3)\n",
        "duplicate match arm for literal `true`",
    );
}

#[test]
fn nested_variant_payload_literals_report_the_missing_shapes() {
    rejects(
        "enum Shape:\n    Circle(int64)\n    Square(int64, int64)\ndef main():\n    shape = Shape.Circle(1)\n    match shape:\n        case Shape.Circle(1):\n            print(1)\n        case Shape.Square(_, 2):\n            print(2)\n",
        "missing `Circle(_)`, `Square(_, _)`",
    );
}

#[test]
fn guarded_non_copy_type_bindings_borrow_for_the_guard() {
    runs(
        "class Holder:\n    x: int64\ndef main():\n    value: Holder | None = Holder(x=1)\n    match value:\n        case Holder as h if h.x > 0:\n            print(h.x)\n        case Holder as h:\n            print(0)\n        case None:\n            print(-1)\n",
        "1\n",
    );
}

#[test]
fn bool_or_pattern_covering_both_literals_makes_the_wildcard_unreachable() {
    rejects(
        "def main():\n    flag = true\n    match flag:\n        case true | false:\n            print(1)\n        case _:\n            print(2)\n",
        "unreachable match arm",
    );
}

#[test]
fn duplicate_and_covered_variant_arms_are_rejected() {
    rejects(
        "def main():\n    value: int64? = Some(1)\n    match value:\n        case Some(x):\n            print(x)\n        case Some(y):\n            print(y)\n        case None:\n            print(0)\n",
        "duplicate match arm for `Option.Some`",
    );
    rejects(
        "enum Shape:\n    Circle(int64)\n    Square(int64, int64)\ndef main():\n    shape = Shape.Circle(1)\n    match shape:\n        case Shape.Circle(_):\n            print(1)\n        case Shape.Circle(1):\n            print(2)\n        case Shape.Square(_, _):\n            print(3)\n",
        "unreachable match arm",
    );
}

#[test]
fn tuple_rows_over_union_and_nested_columns_are_exhaustive() {
    runs(
        "def main():\n    value: int64 | None = 1\n    flag = true\n    match (value, flag):\n        case (None, true) | (None, false):\n            print(1)\n        case (int64 as n, _):\n            print(n)\n",
        "1\n",
    );
    runs(
        "enum Shape:\n    Circle(int64)\n    Square(int64, int64)\ndef main():\n    flag = true\n    pair = ((1, 2), flag)\n    match pair:\n        case ((a, b), true):\n            print(a + b)\n        case (_, false):\n            print(0)\n    nested = (Shape.Circle(1), flag)\n    match nested:\n        case (Shape.Circle(_), _):\n            print(1)\n        case (Shape.Square(_, _), true):\n            print(2)\n        case (Shape.Square(_, _), false):\n            print(3)\n",
        "3\n1\n",
    );
}

// ---------------------------------------------------------------------------
// sema/properties.rs
// ---------------------------------------------------------------------------

#[test]
fn clone_obligations_walk_union_members_of_generic_element_types() {
    runs(
        "def dup[T](values: list[T | None]) -> list[T | None]:\n    return values.copy()\ndef main():\n    items: list[int64 | None] = [1, None]\n    print(dup(items).len())\n",
        "2\n",
    );
}

#[test]
fn non_cloneable_callables_are_found_inside_unions_and_type_arguments() {
    let expected = "would duplicate non-cloneable closure `Callable[def() -> int64]`";
    rejects(
        "type Cb = Callable[def() -> int64]\ndef main():\n    items: list[Cb | None] = [None]\n    copy = items.copy()\n    print(copy.len())\n",
        expected,
    );
    rejects(
        "type Cb = Callable[def() -> int64]\ndef main():\n    items: list[Cb] = [Cb(lambda: 1)]\n    copy = items.copy()\n    print(copy.len())\n",
        expected,
    );
}

#[test]
fn symbolic_copy_shapes_treat_union_and_callable_fields_as_moves() {
    let union = rejects(
        "class Box[T]:\n    value: T | None\ndef main():\n    b = Box[int64](value=1)\n    c = b\n    print(b.value)\n",
        "use of moved value `b`",
    );
    assert_eq!(union.code, "AU3001");
    rejects(
        "type Cb = Callable[def() -> int64]\nclass Box[T]:\n    cb: T\ndef main():\n    b = Box[Cb](cb=Cb(lambda: 1))\n    c = b\n    print(b.cb())\n",
        "use of moved value `b`",
    );
}

// The task-transfer success cases below wait on a generous deadline: the
// task completes immediately, so a bound only matters when host scheduling
// starves it, and a one-second bound has selected the fallback branch under
// load. Each fallback is distinct from the transferred result so the output
// proves which branch ran.
#[test]
fn task_results_of_unit_function_and_stored_task_callable_types_transfer() {
    accepts(
        "def make() -> None:\n    pass\ndef main():\n    with TaskGroup() as group:\n        task = group.start(make)\n        task.result_or(None, timeout=30s)\n",
    );
    runs(
        "def twice(v: int64) -> int64:\n    return v * 2\ndef thrice(v: int64) -> int64:\n    return v * 3\ndef make() -> def(int64) -> int64:\n    return twice\ndef main():\n    with TaskGroup() as group:\n        task = group.start(make)\n        f = task.result_or(thrice, timeout=30s)\n        print(f(2))\n",
        "4\n",
    );
    runs(
        "type Job = TaskCallable[def() -> int64]\ndef make() -> Job:\n    return Job(lambda: 1)\ndef main():\n    with TaskGroup() as group:\n        task = group.start(make)\n        job = task.result_or(Job(lambda: 2), timeout=30s)\n        print(job())\n",
        "1\n",
    );
}

#[test]
fn task_observation_shapes_merge_union_result_members() {
    runs(
        "def make() -> int64 | None:\n    return 1\ndef main():\n    with TaskGroup() as group:\n        task = group.start(make)\n        print(task.result_or(None, timeout=30s))\n",
        "1\n",
    );
}

// ---------------------------------------------------------------------------
// sema/program.rs
// ---------------------------------------------------------------------------

#[test]
fn tuple_alias_is_not_a_constructor() {
    rejects(
        "type Tup = (int64, str)\ndef main():\n    t = Tup(1, \"a\")\n    print(t[0])\n",
        "unsupported call target",
    );
}

#[test]
fn class_field_defaults_cannot_call_module_functions() {
    let error = rejects(
        "class C:\n    x: int64 = make()\ndef make() -> int64:\n    return 1\ndef main():\n    pass\n",
        "unsupported call target",
    );
    assert_eq!(error.span, Some(Span::new(2, 16)));
}

#[test]
fn type_alias_cannot_reuse_a_class_name() {
    rejects(
        "class A:\n    x: int64\ntype A = int64\ndef main():\n    pass\n",
        "duplicate item `A` (previously declared as class at 1:1)",
    );
}

// ---------------------------------------------------------------------------
// sema/traits.rs
// ---------------------------------------------------------------------------

#[test]
fn ambiguous_bound_methods_fall_back_to_the_unsupported_call_diagnostic() {
    rejects(
        "trait A:\n    def name(self) -> str\ntrait B:\n    def name(self) -> str\ndef show[T: A + B](value: T) -> str:\n    return value.name()\ndef main():\n    pass\n",
        "unsupported method call `name` on `T`",
    );
}

#[test]
fn diamond_supertraits_are_collected_once() {
    runs(
        "trait A:\n    def a(self) -> int64\ntrait B: A:\n    def b(self) -> int64\ntrait C: A:\n    def c(self) -> int64\ntrait D: B, C:\n    def d(self) -> int64\nclass H:\n    x: int64\nimpl A for H:\n    def a(self) -> int64:\n        return 1\nimpl B for H:\n    def b(self) -> int64:\n        return 2\nimpl C for H:\n    def c(self) -> int64:\n        return 3\nimpl D for H:\n    def d(self) -> int64:\n        return 4\ndef total[T: D](value: T) -> int64:\n    return value.a() + value.d()\ndef main():\n    print(total(H(x=1)))\n",
        "5\n",
    );
}

#[test]
fn try_conversion_skips_from_impls_with_a_different_source() {
    rejects(
        "trait From[T]:\n    def from(value: own T) -> Self\nenum ReadError:\n    Missing\nenum OtherError:\n    Bad\nenum AppError:\n    Read(ReadError)\nimpl From[ReadError] for AppError:\n    def from(value: own ReadError) -> AppError:\n        return AppError.Read(value)\ndef read(flag: bool) -> Result[int32, OtherError]:\n    return Result.Err(OtherError.Bad)\ndef load(flag: bool) -> Result[int32, AppError]:\n    value = try read(flag)\n    return Result.Ok(value + 1)\ndef main():\n    print(load(true))\n",
        "`try` error type `OtherError` does not match enclosing `Result` error type `AppError`",
    );
}

#[test]
fn bitwise_not_has_no_operator_trait() {
    rejects(
        "class V:\n    x: int64\ndef main():\n    v = V(x=1)\n    w = ~v\n",
        "unary `~` expects an integer value, found `V`",
    );
}

#[test]
fn operator_dispatch_through_a_bound_maps_trait_level_clone_obligations() {
    runs(
        "trait Add[Rhs]:\n    def add(self, other: own Rhs) -> list[Rhs]:\n        return [other].copy()\nclass H:\n    x: int64\nimpl Add[H] for H:\n    pass\ndef combine[T: Add[T]](a: T, b: own T) -> list[T]:\n    return a + b\ndef main():\n    print(combine(H(x=1), H(x=2)).len())\n",
        "1\n",
    );
}

#[test]
fn bound_method_dispatch_maps_trait_level_array_equality_obligations() {
    let error = rejects(
        "trait Equaler[T]:\n    def equal(self, left: T, right: T) -> bool:\n        return left == right\nclass Matcher[T]:\n    pass\nimpl[T] Equaler[T] for Matcher[T]:\n    pass\ndef compare[T, M: Equaler[T]](matcher: M, left: T, right: T) -> bool:\n    return matcher.equal(left, right)\ndef main():\n    matcher = Matcher[Array[int32]]()\n    left = Array[int32].zeros([1])\n    right = Array[int32].zeros([1])\n    print(compare[Array[int32], Matcher[Array[int32]]](matcher, left, right))\n",
        "whose equality is unavailable",
    );
    assert_eq!(error.code, "AU2003");
}

#[test]
fn union_arguments_satisfy_bounds_on_imported_traits_with_imported_implementations() {
    // The bound, the trait, the implementations, and the bounded function all
    // live in the imported module, so the caller's checker resolves the bound
    // and the implementations' traits through the module registry, and a
    // method value bound from an imported implementation exposes the imported
    // trait's contract.
    let temp = TempDir::new("aura-sema-imported-union-bound");
    temp.write(
        "api.au",
        "public trait Named:\n    def name(self) -> str\n\npublic class Dog:\n    public tag: str\n\npublic class Cat:\n    public tag: str\n\nimpl Named for Dog:\n    def name(self) -> str:\n        return self.tag.clone()\n\nimpl Named for Cat:\n    def name(self) -> str:\n        return self.tag.clone()\n\npublic def show[T: Named](value: T):\n    print(value.name())\n",
    );
    let main_path = temp.write(
        "main.au",
        "import api\n\ndef main():\n    pet: api.Dog | api.Cat = api.Dog(tag=\"rex\")\n    api.show(pet)\n    other: api.Dog | api.Cat = api.Cat(tag=\"tom\")\n    api.show(other)\n    dog = api.Dog(tag=\"bound\")\n    named = dog.name\n    print(named())\n",
    );
    let output = run_path(&main_path).expect("an imported all-member union satisfies the bound");
    assert_eq!(output.stdout, "rex\ntom\nbound\n");
}
