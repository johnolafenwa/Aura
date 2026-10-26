//! Allocation: build and walk many complete binary trees (Benchmarks Game).
//!
//! Builds a stretch tree of depth N + 1, keeps one long-lived tree of depth N, and
//! for each even depth from 4 to N builds 2^(N - depth + 4) short-lived trees,
//! counting the nodes of each. It measures allocation, recursion, and freeing.

enum Tree {
    Leaf,
    Node(Box<Tree>, Box<Tree>),
}

fn make(depth: i64) -> Tree {
    if depth == 0 {
        return Tree::Leaf;
    }
    Tree::Node(Box::new(make(depth - 1)), Box::new(make(depth - 1)))
}

fn check(tree: &Tree) -> i64 {
    match tree {
        Tree::Leaf => 1,
        Tree::Node(left, right) => 1 + check(left) + check(right),
    }
}

fn main() {
    let n: i64 = std::env::args()
        .nth(1)
        .unwrap()
        .parse::<i64>()
        .unwrap()
        .max(6);
    let stretch = check(&make(n + 1));
    let long_lived = make(n);
    let mut rows = Vec::new();
    let mut depth = 4;
    while depth <= n {
        let iterations = 1i64 << (n - depth + 4);
        let mut total = 0;
        for _ in 0..iterations {
            total += check(&make(depth));
        }
        rows.push(format!("{depth}:{iterations}:{total}"));
        depth += 2;
    }
    println!(
        "binary_trees depth={n} stretch={stretch} rows={} long_lived={}",
        rows.join(","),
        check(&long_lived)
    );
}
