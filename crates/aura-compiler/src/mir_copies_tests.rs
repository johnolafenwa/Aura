use super::{body_only_borrows, borrowed_element_method, lendable_loop_element};
use crate::ast::Item;
use crate::sema::Type;

fn named(name: &str, args: Vec<Type>) -> Type {
    Type::Named(name.to_string(), args)
}

fn text() -> Type {
    named("str", Vec::new())
}

/// Whether the body of each function in `source` only borrows `x`, in order.
fn borrows(source: &str, element: &Type) -> Vec<(String, bool)> {
    let module = crate::parser::parse(source).expect("the cases should parse");
    module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Function(function) => Some((
                function.name.clone(),
                body_only_borrows(&function.body, "x", element),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn lendable_elements_are_text_and_collections() {
    assert!(lendable_loop_element(&text()));
    assert!(lendable_loop_element(&named("list", vec![text()])));
    assert!(lendable_loop_element(&named("set", vec![text()])));
    assert!(lendable_loop_element(&named("dict", vec![text(), text()])));
    assert!(!lendable_loop_element(&named("int64", Vec::new())));
    assert!(!lendable_loop_element(&named("list", Vec::new())));
    assert!(!lendable_loop_element(&Type::Tuple(vec![text()])));
    let dict = named("dict", vec![text(), text()]);
    assert!(borrowed_element_method(&dict, "get", 1));
    assert!(borrowed_element_method(&dict, "len", 0));
    assert!(!borrowed_element_method(&text(), "get", 1));
    assert!(!borrowed_element_method(&text(), "upper", 0));
}

#[test]
fn only_comparisons_len_and_get_borrow_the_binding() {
    let source = r#"
def compare_and_measure():
    if x == "a" or (x) != y:
        pass
    assert x.len() > 0, f"{n}"
    while n < x.len():
        break
    return x.get("k")

def statements_without_the_binding():
    total = [1, 2][0] + {"a": 1}["a"]
    view v = items
    a, b = pair
    for item in items:
        continue
    with open_thing() as handle:
        pass
    match n:
        case 1 if n > 0:
            pass
    other.field = 1
    other[n] = 2
    print(-n, n as float64, n is None, items[1:2], (1, 2), {1}, f"{n:>4}")
    print(lambda value: value, [value for value in items], 1 if flag else 2)
    print(1 < n < 3, n in items, 1.5, true, 2s)
    return

def passes_the_binding():
    use(x)

def assigns_the_binding():
    x = "b"

def writes_through_the_binding():
    x.field = 1

def indexes_through_the_binding():
    x[0] = 1

def views_the_binding():
    view v = x

def destructures_into_the_binding():
    x, y = pair

def loops_into_the_binding():
    for x in items:
        pass

def manages_into_the_binding():
    with thing() as x:
        pass

def captures_the_binding():
    f = lambda value: x

def comprehends_the_binding():
    f = [x for y in items]

def calls_another_method():
    print(x.upper())

def orders_the_binding():
    print(x < "b")
"#;
    let results = borrows(source, &named("dict", vec![text(), text()]));
    let expected = [
        ("compare_and_measure", true),
        ("statements_without_the_binding", true),
        ("passes_the_binding", false),
        ("assigns_the_binding", false),
        ("writes_through_the_binding", false),
        ("indexes_through_the_binding", false),
        ("views_the_binding", false),
        ("destructures_into_the_binding", false),
        ("loops_into_the_binding", false),
        ("manages_into_the_binding", false),
        ("captures_the_binding", false),
        ("comprehends_the_binding", false),
        ("calls_another_method", false),
        ("orders_the_binding", false),
    ];
    for ((name, borrowed), (expected_name, expected_borrowed)) in results.iter().zip(expected) {
        assert_eq!(name, expected_name);
        assert_eq!(*borrowed, expected_borrowed, "{name}");
    }
    assert_eq!(results.len(), expected.len());
}
