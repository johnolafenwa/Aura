//! Unit tests of the extended fast iteration forms: function copies that
//! take a generator's arguments, and frames with collection, class, and
//! generator fields.

use super::iterator_fast::{self, Scope};
use super::iterator_fast_args::{specialize, WITH_PREFIX};
use super::{Item, Module, ModuleContext};

const HEADER: &str = "from iter import Generator\n\n\
def upto(n: int64) -> Generator[int64]:\n    mut i = 0\n    while i < n:\n        yield i\n        i += 1\n\n\
def names(n: int64) -> Generator[str]:\n    for i in range(n):\n        yield f\"{i}\"\n\n";

fn parse(body: &str) -> Module {
    crate::parser::parse(&format!("{HEADER}{body}")).expect("test source parses")
}

fn function_names(module: &Module) -> Vec<String> {
    module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Function(function) => Some(function.name.clone()),
            _ => None,
        })
        .collect()
}

fn copies(module: &Module) -> Vec<String> {
    function_names(module)
        .into_iter()
        .filter(|name| name.starts_with(WITH_PREFIX))
        .collect()
}

fn specialized(body: &str) -> (usize, Vec<String>) {
    let mut module = parse(body);
    let mut columns = 0;
    let count = specialize(&mut module, &mut columns);
    (count, copies(&module))
}

#[test]
fn a_loop_over_an_owned_generator_parameter_gets_a_copy() {
    let (count, names) = specialized(
        "def total(source: own Generator[int64]) -> int64:\n    mut t = 0\n    for x in source:\n        t += x\n    return t\n\n\
         def main():\n    print(total(upto(3)))\n    print(total(upto(4)))\n",
    );
    assert_eq!(count, 2);
    assert_eq!(names, ["__aura_with_total__0_upto"]);
}

#[test]
fn parameters_used_any_other_way_keep_the_generator() {
    // Each consumer uses its generator in a way a copy cannot follow: more
    // than once, in a nested loop, borrowed, rebound, or shadowed.
    let consumers = [
        "def twice(source: own Generator[int64]) -> int64:\n    for x in source:\n        pass\n    for y in source:\n        pass\n    return 0\n",
        "def nested(source: own Generator[int64]) -> int64:\n    for i in range(2):\n        for x in source:\n            pass\n    return 0\n",
        "def inwhile(source: own Generator[int64]) -> int64:\n    while true:\n        for x in source:\n            pass\n        break\n    return 0\n",
        "def borrowed(source: Generator[int64]) -> int64:\n    for x in source:\n        pass\n    return 0\n",
        "def mutated(source: own Generator[int64]) -> int64:\n    for x in mut source:\n        pass\n    return 0\n",
        "def rebound(source: own Generator[int64]) -> int64:\n    source = upto(1)\n    for x in source:\n        pass\n    return 0\n",
        "def viewed(source: own Generator[int64]) -> int64:\n    view source = upto(1)\n    for x in source:\n        pass\n    return 0\n",
        "def destructured(source: own Generator[int64]) -> int64:\n    (source, other) = (upto(1), 2)\n    for x in source:\n        pass\n    return 0\n",
        "def asserted(source: own Generator[int64]) -> int64:\n    assert source is not None, \"x\"\n    for x in source:\n        pass\n    return 0\n",
        "def returned(source: own Generator[int64]) -> Generator[int64]:\n    for x in source:\n        pass\n    return source\n",
        "def called(source: own Generator[int64]) -> int64:\n    print(source)\n    for x in source:\n        pass\n    return 0\n",
        "def targeted(source: own Generator[int64]) -> int64:\n    for source in range(3):\n        pass\n    return 0\n",
        "def lambdas(source: own Generator[int64]) -> int64:\n    f = lambda: source\n    for x in source:\n        pass\n    return 0\n",
        "def matched(source: own Generator[int64]) -> int64:\n    match 1:\n        case source:\n            pass\n    for x in source:\n        pass\n    return 0\n",
        "def guarded(source: own Generator[int64]) -> int64:\n    match 1:\n        case 1 if source is None:\n            pass\n        case _:\n            for x in source:\n                pass\n    return 0\n",
        "def managed(source: own Generator[int64]) -> int64:\n    with open_thing() as source:\n        pass\n    for x in source:\n        pass\n    return 0\n",
        "def scrutinized(source: own Generator[int64]) -> int64:\n    match source:\n        case _:\n            pass\n    return 0\n",
        "def condition(source: own Generator[int64]) -> int64:\n    if source is None:\n        pass\n    elif true:\n        for x in source:\n            pass\n    else:\n        pass\n    return 0\n",
        "def indexed(source: own Generator[int64]) -> int64:\n    mut xs = [1]\n    xs[0] = 2\n    xs.y = source\n    for x in source:\n        pass\n    return 0\n",
        "def element(source: own Generator[int64]) -> int64:\n    mut xs = [1]\n    xs[source] = 2\n    for x in source:\n        pass\n    return 0\n",
        "def invariant(source: own Generator[int64]) -> int64:\n    mut i = 0\n    while i < 1:\n        invariant i >= 0\n        i += 1\n    for x in source:\n        invariant true\n        pass\n    return 0\n",
        "def sourced(source: own Generator[int64]) -> int64:\n    for x in wrap(source):\n        pass\n    return 0\n",
    ];
    for consumer in consumers {
        let name = consumer
            .strip_prefix("def ")
            .and_then(|rest| rest.split('(').next())
            .expect("consumers are functions");
        let (_, names) = specialized(&format!(
            "{consumer}\ndef main():\n    print({name}(upto(3)))\n"
        ));
        assert!(names.is_empty(), "{name} got a copy: {names:?}");
    }
}

#[test]
fn a_copy_needs_a_matching_item_type_and_plain_calls() {
    let consumer = "def total(source: own Generator[int64]) -> int64:\n    for x in source:\n        pass\n    return 0\n\n";
    for call in [
        "total(names(3))",
        "total(source=upto(3))",
        "total(upto(n=3))",
        "total(upto(3), 4)",
        "total(other(3))",
        "total(local(3))",
        "upto(upto(3))",
    ] {
        let (_, names) = specialized(&format!(
            "{consumer}def main():\n    local = upto\n    print({call})\n"
        ));
        assert!(names.is_empty(), "{call} got a copy: {names:?}");
    }
    // A function that is not plain is left alone.
    for header in [
        "def total[T](source: own Generator[int64]) -> int64:",
        "@trace\ndef total(source: own Generator[int64]) -> int64:",
        "def total(source: own Generator[int64], k: int64 = 1) -> int64:",
    ] {
        let (_, names) = specialized(&format!(
            "{header}\n    for x in source:\n        pass\n    return 0\n\ndef main():\n    print(total(upto(3)))\n"
        ));
        assert!(names.is_empty(), "{header} got a copy: {names:?}");
    }
}

#[test]
fn pipelines_held_locals_and_self_calls_get_copies() {
    let (_, names) = specialized(
        "def doubled(source: own Generator[int64]) -> Generator[int64]:\n    for x in source:\n        yield x * 2\n\n\
         def total(source: own Generator[int64]) -> int64:\n    mut t = 0\n    for x in source:\n        t += x\n    return t\n\n\
         def again(source: own Generator[int64], depth: int64) -> int64:\n    if depth > 0:\n        return again(upto(depth), depth - 1)\n    for x in source:\n        pass\n    return 0\n\n\
         class Box:\n    def run(self) -> int64:\n        g = upto(2)\n        return total(g)\n\n\
         def main():\n    print(total(doubled(upto(3))))\n    g = upto(5)\n    print(1)\n    print(total(g))\n    h = upto(2)\n    print(h)\n    print(total(h))\n    print(again(upto(1), 2))\n    if true:\n        k = upto(1)\n        print(total(k))\n    print(total(upto(1)) + total(upto(2)))\n\n\
         print(total(upto(9)))\n",
    );
    assert_eq!(
        names,
        [
            "__aura_with_again__0_upto",
            "__aura_with_doubled__0_upto",
            "__aura_with_total__0___aura_with_doubled__0_upto",
            "__aura_with_total__0_upto",
        ]
    );
}

#[test]
fn held_generators_move_only_into_a_direct_call() {
    let consumer = "def total(source: own Generator[int64]) -> int64:\n    for x in source:\n        pass\n    return 0\n\n";
    for main in [
        // Read twice, passed in a loop, passed in a lambda, passed to a
        // method, or passed under a rebound name.
        "    g = upto(1)\n    print(g)\n    print(total(g))\n",
        "    g = upto(1)\n    for i in range(2):\n        print(total(g))\n",
        "    g = upto(1)\n    f = lambda: total(g)\n",
        "    g = upto(1)\n    x.total(g)\n",
        "    g = upto(1)\n    total = len\n    print(total(g))\n",
        "    g: Generator[int64] = upto(1)\n    print(total(g))\n",
        "    g = upto(n=1)\n    print(total(g))\n",
        "    g = upto(1)\n    print(total(g, 2))\n",
        "    g = upto(1)\n    print(other(g))\n",
    ] {
        let (_, names) = specialized(&format!("{consumer}def main():\n{main}"));
        assert!(names.is_empty(), "{main} got a copy: {names:?}");
    }
}

#[test]
fn modules_without_generators_or_consumers_are_unchanged() {
    for body in [
        "def main():\n    print(upto(3))\n",
        "def total(source: own Generator[int64]) -> int64:\n    for x in source:\n        pass\n    return 0\n",
    ] {
        let mut module = parse(body);
        let before = function_names(&module);
        let mut columns = 0;
        assert_eq!(specialize(&mut module, &mut columns), 0);
        assert_eq!(function_names(&module), before);
    }
    let mut module = crate::parser::parse(
        "def total(source: own Generator[int64]) -> int64:\n    for x in source:\n        pass\n    return 0\n",
    )
    .expect("parses");
    let mut columns = 0;
    assert_eq!(specialize(&mut module, &mut columns), 0);
}

#[test]
fn type_keys_round_trip_and_reject_what_frames_cannot_name() {
    let module = crate::parser::parse(
        "class P:\n    x: int64\n    name: str = \"p\"\n    tags: list[str]\n\n\
         class Q:\n    p: P\n    seen: set[int64]\n    counts: dict[str, int64]\n\n\
         class R:\n    next: indirect R | None\n\n\
         class S[T]:\n    item: T\n\n\
         class U:\n    pair: (int64, int64)\n\n\
         def f(a: list[dict[str, list[int64]]], b: (int64, str), c: int64 | None) -> int64:\n    return 0\n",
    )
    .expect("parses");
    let Some(Item::Function(function)) = module.items.last() else {
        panic!("the last item is a function");
    };
    let keys = function
        .params
        .iter()
        .map(|param| iterator_fast::type_key(&param.ty))
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [Some("list[dict[str, list[int64]]]".to_string()), None, None]
    );
    let key = keys[0].as_deref().expect("key");
    assert_eq!(
        iterator_fast::type_key(&iterator_fast::key_type(key, function.span)).as_deref(),
        Some(key)
    );
    assert_eq!(
        iterator_fast::parse_key("dict[str, list[int64]]"),
        Some((
            "dict".to_string(),
            vec!["str".to_string(), "list[int64]".to_string()]
        ))
    );
    assert_eq!(iterator_fast::parse_key("list[int64"), None);
    assert_eq!(iterator_fast::parse_key("list[int64]]"), None);

    let scope = Scope::of(&module, true);
    for (key, holds) in [
        ("int64", true),
        ("str", true),
        ("P", true),
        ("Q", true),
        ("list[P]", true),
        ("dict[str, list[int64]]", true),
        ("set[str]", true),
        ("R", false),
        ("S", false),
        ("U", false),
        ("list[int64, int64]", false),
        ("list[R]", false),
        ("tuple[int64]", false),
        ("__aura_frame_missing", false),
        ("Missing", false),
    ] {
        assert_eq!(scope.holds(key), holds, "{key}");
    }
    let basic = Scope::of(&module, false);
    assert!(basic.holds("int64") && basic.holds("str"));
    assert!(!basic.holds("P") && !basic.holds("list[int64]"));
    assert_eq!(Scope::indexed_item("list[str]").as_deref(), Some("str"));
    assert_eq!(Scope::indexed_item("list[P]"), None);
    assert_eq!(Scope::indexed_item("set[int64]"), None);
    assert_eq!(Scope::element("list[P]").as_deref(), Some("P"));
    assert_eq!(Scope::element("dict[str, P]"), None);
}

#[test]
fn extended_forms_fall_back_to_the_basic_forms() {
    let context = ModuleContext::default();
    // Only basic forms: the extended expansion steps aside.
    let basic_only = parse("def main():\n    for x in upto(3):\n        print(x)\n");
    assert!(iterator_fast::expand(&basic_only, &context, true).is_none());
    assert!(iterator_fast::expand(&basic_only, &context, false).is_some());
    // A list parameter needs the extended form.
    let extended = parse(
        "def each(xs: own list[int64]) -> Generator[int64]:\n    for x in xs:\n        yield x\n        yield x\n\n\
         def main():\n    for x in each([1, 2]):\n        print(x)\n",
    );
    let (module, _) = iterator_fast::expand(&extended, &context, true).expect("extended");
    assert!(function_names(&module).contains(&"__aura_frame_new_each".to_string()));
    let basic = iterator_fast::expand(&extended, &context, false)
        .map(|(module, _)| function_names(&module))
        .unwrap_or_default();
    assert!(!basic.contains(&"__aura_frame_new_each".to_string()));
}
