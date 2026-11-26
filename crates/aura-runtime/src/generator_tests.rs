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
fn isolated_recovery_releases_abandoned_work_if_its_stack_cannot_start() {
    use super::{run_isolated, IsolatedFailure, DIRECT_RECOVER_STACK_BYTES};

    let abandoned = std::cell::Cell::new(0);
    crate::runtime_value::fail_next_lightweight_task_stack_allocation();
    let result = run_isolated(
        || panic!("a body without a stack must not start"),
        DIRECT_RECOVER_STACK_BYTES,
        || abandoned.set(abandoned.get() + 1),
    );
    let Err(IsolatedFailure::Trap(diagnostic)) = result else {
        panic!("expected a stack allocation diagnostic");
    };
    assert_eq!(diagnostic.code, "AU4005");
    assert!(diagnostic
        .message
        .contains("cannot allocate a stack for `control.recover`"));
    assert_eq!(abandoned.get(), 1);
    assert_eq!(super::running_generator_depth(), 0);
}

#[test]
fn isolated_recovery_diagnoses_an_unmatched_yield_and_restores_the_outer_stack() {
    use super::{run_isolated, IsolatedFailure, DIRECT_RECOVER_STACK_BYTES};

    let abandoned = std::cell::Cell::new(0);
    let result = run_isolated(
        || {
            assert_eq!(super::running_generator_depth(), 1);
            generator_yield(int(9))?;
            panic!("an unmatched yield should abandon the recovered body");
        },
        DIRECT_RECOVER_STACK_BYTES,
        || abandoned.set(abandoned.get() + 1),
    );
    let Err(IsolatedFailure::Trap(diagnostic)) = result else {
        panic!("expected an unmatched yield diagnostic");
    };
    assert_eq!(diagnostic.code, "AU4010");
    assert!(diagnostic.message.contains("outside a generator body"));
    assert_eq!(abandoned.get(), 1);
    assert_eq!(super::running_generator_depth(), 0);
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

#[test]
fn a_bulk_release_discards_a_suspended_generator_without_running_it() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let generator = counting(5, log.clone());
    assert!(generator.resume().expect("resume"));
    let kept = super::without_generator_drop_close(|| {
        drop(generator);
        log.borrow().len()
    });
    assert_eq!(kept, 1, "the body did not run again");
}

#[test]
fn a_stack_that_cannot_be_reserved_fails_the_first_resume() {
    let generator = GeneratorValue::new(|| Ok(()), usize::MAX / 2, true, None);
    let error = generator.resume().expect_err("no stack");
    assert!(
        error.message.contains("cannot allocate a generator stack"),
        "{error:?}"
    );
}

#[test]
fn closing_a_fresh_generator_ends_it_without_running_the_body() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let generator = counting(2, log.clone());
    generator.close().expect("close");
    assert!(!generator.resume().expect("a closed generator has ended"));
    assert!(log.borrow().is_empty(), "the body never ran");
}

#[test]
fn a_generator_resumes_only_on_the_thread_that_made_it() {
    let generator = GeneratorValue::new(|| Ok(()), DIRECT_GENERATOR_STACK_BYTES, true, None);
    let error = std::thread::spawn(move || generator.resume().expect_err("another thread"))
        .join()
        .expect("the thread finishes");
    assert!(error.message.contains("outside the task"), "{error:?}");
}

#[test]
fn dropping_a_suspended_generator_without_unwinding_resets_its_stack() {
    let generator = GeneratorValue::new(
        || {
            generator_yield(int(1))?;
            Ok(())
        },
        DIRECT_GENERATOR_STACK_BYTES,
        false,
        None,
    );
    assert!(generator.resume().expect("resume"));
    super::without_generator_drop_close(|| drop(generator));
}

#[test]
fn a_scheduler_wait_outside_a_generator_goes_back_to_the_task() {
    let wait = super::super::TaskYield::YieldNow;
    assert!(super::generator_wait(wait).is_err(), "no body is running");
}

#[test]
fn a_finished_generator_returns_its_stack_to_the_task_stack_pool() {
    // An unusual size, so other tests' stacks never match it.
    let size = DIRECT_GENERATOR_STACK_BYTES + 7 * 4096;
    let pooled = || {
        super::super::TASK_STACK_POOL.with(|pool| {
            super::super::lock_mutex(pool)
                .stacks
                .iter()
                .filter(|entry| entry.size == size)
                .count()
        })
    };
    let run = |yields: bool| {
        let generator = GeneratorValue::new(
            move || {
                if yields {
                    generator_yield(int(1))?;
                }
                Ok(())
            },
            size,
            true,
            None,
        );
        while generator.resume().expect("resume") {}
    };
    run(false);
    assert_eq!(pooled(), 1, "the ended body's stack is pooled");
    run(true);
    assert_eq!(pooled(), 1, "the next generator reused it");

    // A generator dropped while suspended is unwound, then pooled.
    let generator = GeneratorValue::new(
        || {
            generator_yield(int(1))?;
            Ok(())
        },
        size,
        true,
        None,
    );
    assert!(generator.resume().expect("resume"));
    assert_eq!(pooled(), 0, "the running body holds the stack");
    super::without_generator_drop_close(|| drop(generator));
    assert_eq!(pooled(), 1);
}

#[test]
fn a_weak_handle_upgrades_only_while_a_strong_handle_remains() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let generator = counting(3, log.clone());
    let weak = generator.downgrade();
    let upgraded = weak.clone().upgrade().expect("the generator is alive");
    assert_eq!(upgraded, generator);
    assert!(upgraded
        .resume()
        .expect("resume through the upgraded handle"));
    assert_eq!(generator.take_item().expect("shared state"), int(0));
    drop(upgraded);
    drop(generator);
    assert!(
        weak.upgrade().is_none(),
        "a weak handle keeps nothing alive"
    );
    assert_eq!(*log.borrow(), ["start", "closed"]);
}

#[test]
fn dropping_a_suspended_generator_whose_close_fails_does_not_unwind() {
    let closes = Rc::new(RefCell::new(0));
    let seen = closes.clone();
    let generator = GeneratorValue::new(
        move || {
            while !generator_yield(int(1))? {
                *seen.borrow_mut() += 1;
            }
            Ok(())
        },
        DIRECT_GENERATOR_STACK_BYTES,
        true,
        None,
    );
    assert!(generator.resume().expect("resume"));
    // Outside any runtime boundary the close failure has nowhere to go and
    // is dropped instead of escaping `Drop`.
    drop(generator);
    assert_eq!(
        *closes.borrow(),
        1,
        "the drop ran the body's close path once"
    );
}
