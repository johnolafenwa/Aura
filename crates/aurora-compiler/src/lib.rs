pub mod ast;
pub mod diag;
pub mod interpreter;
pub mod lexer;
pub mod parser;
pub mod sema;

pub use diag::{Diagnostic, Result, Span};
pub use interpreter::{run, RunOutput, Value};
pub use sema::Program;

pub fn parse_source(source: &str) -> Result<ast::Module> {
    parser::parse(source)
}

pub fn check_source(source: &str) -> Result<Program> {
    let module = parse_source(source)?;
    sema::check(module)
}

pub fn run_source(source: &str) -> Result<RunOutput> {
    let program = check_source(source)?;
    run(&program)
}

#[cfg(test)]
mod tests {
    use super::{check_source, parse_source, run_source, Value};

    const POINT_SOURCE: &str = include_str!("../../../examples/point.au");

    #[test]
    fn parses_the_point_milestone() {
        let module = parse_source(POINT_SOURCE).expect("point program should parse");
        assert_eq!(module.items.len(), 3);
    }

    #[test]
    fn type_checks_the_point_milestone() {
        check_source(POINT_SOURCE).expect("point program should type-check");
    }

    #[test]
    fn runs_the_point_milestone() {
        let output = run_source(POINT_SOURCE).expect("point program should run");
        assert_eq!(output.stdout, "5\n");
        assert_eq!(output.value, Value::Int(0));
    }
}
