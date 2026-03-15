use std::fs;
use std::process;

use aurora_compiler::{check_source, parse_source, run_source, Value};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        print_usage_and_exit();
    };
    let Some(path) = args.next() else {
        print_usage_and_exit();
    };

    if args.next().is_some() {
        print_usage_and_exit();
    }

    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("failed to read `{}`: {}", path, error);
            process::exit(1);
        }
    };

    match command.as_str() {
        "check" => match check_source(&source) {
            Ok(_) => {
                println!("ok");
            }
            Err(error) => {
                eprintln!("{}: {}", path, error);
                process::exit(1);
            }
        },
        "run" => match run_source(&source) {
            Ok(output) => {
                print!("{}", output.stdout);
                if let Value::Int(code) = output.value {
                    process::exit(code as i32);
                }
            }
            Err(error) => {
                eprintln!("{}: {}", path, error);
                process::exit(1);
            }
        },
        "ast" => match parse_source(&source) {
            Ok(module) => {
                println!("{:#?}", module);
            }
            Err(error) => {
                eprintln!("{}: {}", path, error);
                process::exit(1);
            }
        },
        _ => print_usage_and_exit(),
    }
}

fn print_usage_and_exit() -> ! {
    eprintln!("usage: aura <check|run|ast> <file.au>");
    process::exit(2);
}
