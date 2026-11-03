use super::{generator_yield, GeneratorHooks, GeneratorValue, DIRECT_GENERATOR_STACK_BYTES};
use crate::diag::Diagnostic;
use crate::runtime_value::Value;
use std::cell::RefCell;
use std::rc::Rc;

fn int(value: i64) -> Value {
    Value::Int(crate::integer::IntegerValue::from_signed(i128::from(value)))
}

fn counting(n: i64, log: Rc<RefCell<Vec<String>>>) -> GeneratorValue {
    GeneratorValue::new(
        move || {
            log.borrow_mut().push("start".to_string());
            for i in 0..n {
                if !generator_yield(int(i))? {
                    log.borrow_mut().push("closed".to_string());
                    return Ok(());
                }
            }
            log.borrow_mut().push("done".to_string());
            Ok(())
        },
        DIRECT_GENERATOR_STACK_BYTES,
        true,
        None,
    )
}

#[test]
fn a_generator_runs_lazily_to_each_yield_and_then_ends() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let generator = counting(2, log.clone());
    assert!(log.borrow().is_empty(), "creating runs nothing");
    assert!(generator.take_item().is_err(), "no item before a resume");
    for expected in 0..2 {
        assert!(generator.resume().expect("resume"));
        assert_eq!(generator.take_item().expect("item"), int(expected));
    }
    assert!(!generator.resume().expect("the body ends"));
    assert!(!generator.resume().expect("an ended generator stays ended"));
    generator
        .close()
        .expect("closing an ended generator does nothing");
    assert_eq!(*log.borrow(), ["start", "done"]);
    assert_eq!(generator, generator.clone());
    assert_eq!(format!("{generator:?}"), "<generator>");
}

#[test]
fn closing_resumes_a_suspended_body_with_yield_returning_false() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let generator = counting(5, log.clone());
    assert!(generator.resume().expect("resume"));
    generator.close().expect("close");
    assert!(!generator.resume().expect("a closed generator ends"));
    assert_eq!(*log.borrow(), ["start", "closed"]);

    let fresh = counting(5, log.clone());
    fresh.close().expect("closing a fresh generator");
    assert!(!fresh.resume().expect("it never starts"));
    assert_eq!(log.borrow().len(), 2);
}

#[test]
fn dropping_the_last_handle_of_a_suspended_generator_closes_it() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let generator = counting(5, log.clone());
    assert!(generator.resume().expect("resume"));
    let other = generator.clone();
    drop(generator);
    assert_eq!(log.borrow().len(), 1, "another handle keeps it open");
    drop(other);
    assert_eq!(*log.borrow(), ["start", "closed"]);
}

#[test]
fn a_failing_body_fails_every_later_resume() {
    let generator = GeneratorValue::new(
        || Err(Diagnostic::coded("AU4001", "broke")),
        DIRECT_GENERATOR_STACK_BYTES,
        true,
        None,
    );
    assert_eq!(generator.resume().expect_err("fails").message, "broke");
    assert_eq!(
        generator.resume().expect_err("still fails").message,
        "broke"
    );
    generator
        .close()
        .expect("closing a failed generator does nothing");
}

#[test]
fn a_body_that_yields_while_closing_fails() {
    let generator = GeneratorValue::new(
        || {
            while !generator_yield(int(1))? {}
            Ok(())
        },
        DIRECT_GENERATOR_STACK_BYTES,
        true,
        None,
    );
    assert!(generator.resume().expect("resume"));
    let error = generator.close().expect_err("it yields during close");
    assert_eq!(error.code, "AU4010");
    assert_eq!(generator.resume().expect_err("failed").code, "AU4010");
}

#[test]
fn yield_outside_a_body_and_self_resume_are_errors() {
    assert_eq!(generator_yield(int(1)).expect_err("no body").code, "AU4010");

    let slot: Rc<RefCell<Option<GeneratorValue>>> = Rc::new(RefCell::new(None));
    let inner = slot.clone();
    let generator = GeneratorValue::new(
        move || {
            let me = inner.borrow().clone().expect("handle");
            let resumed = me.resume().map(|_| ());
            let closed = me.close();
            assert_eq!(resumed.expect_err("self resume").code, "AU4010");
            assert_eq!(closed.expect_err("self close").code, "AU4010");
            Ok(())
        },
        DIRECT_GENERATOR_STACK_BYTES,
        true,
        None,
    );
    *slot.borrow_mut() = Some(generator.clone());
    assert!(!generator.resume().expect("the body ends"));
    slot.borrow_mut().take();
}

#[test]
fn a_panicking_body_fails_and_rethrows() {
    let generator =
        GeneratorValue::new(|| panic!("trap"), DIRECT_GENERATOR_STACK_BYTES, false, None);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| generator.resume()));
    assert!(outcome.is_err());
    assert!(generator.resume().is_err(), "the generator failed");
}

struct Recorder(Rc<RefCell<Vec<&'static str>>>);

impl GeneratorHooks for Recorder {
    fn before_resume(&mut self) {
        self.0.borrow_mut().push("before");
    }

    fn after_suspend(&mut self) {
        self.0.borrow_mut().push("after");
    }
}

#[test]
fn hooks_run_around_every_resume() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let generator = GeneratorValue::new(
        || {
            generator_yield(int(1))?;
            generator_yield(int(2))?;
            Ok(())
        },
        DIRECT_GENERATOR_STACK_BYTES,
        false,
        Some(Box::new(Recorder(events.clone()))),
    );
    assert!(generator.resume().expect("resume"));
    assert_eq!(*events.borrow(), ["before", "after"]);
    // A clone outlives the drop-close, so dropping resets the stack.
    let keep = generator.clone();
    drop(generator);
    drop(keep);
}
