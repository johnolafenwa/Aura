use std::hint::black_box;
use std::time::Instant;

use aura_compiler::{check_source, emit_host_native_object, lower_source_to_mir, parse_source};

const SOURCE: &str = include_str!("../../../examples/traits/specialized_trait_dispatch.au");
const ITERATIONS: usize = 50;

fn measure(mut operation: impl FnMut()) -> u128 {
    let started = Instant::now();
    for _ in 0..ITERATIONS {
        operation();
    }
    started.elapsed().as_micros()
}

fn main() {
    let parse_micros = measure(|| {
        black_box(parse_source(black_box(SOURCE)).expect("benchmark source should parse"));
    });
    let check_micros = measure(|| {
        black_box(check_source(black_box(SOURCE)).expect("benchmark source should check"));
    });
    let lower_micros = measure(|| {
        black_box(lower_source_to_mir(black_box(SOURCE)).expect("benchmark source should lower"));
    });
    let module = lower_source_to_mir(SOURCE).expect("benchmark source should lower");
    let emit_micros = measure(|| {
        black_box(emit_host_native_object(black_box(&module)).expect("benchmark MIR should emit"));
    });

    println!(
        "{{\"iterations\":{ITERATIONS},\"parse_micros\":{parse_micros},\"check_micros\":{check_micros},\"lower_micros\":{lower_micros},\"emit_micros\":{emit_micros}}}"
    );
}
