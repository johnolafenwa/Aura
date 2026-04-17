use super::{
    cast_numeric_value, option_none, option_some, render_float, result_err, result_ok,
    send_error_closed, CancellationContext, ChannelValue, EnumVariantValue, MapValue, RangeValue,
    SetValue, TaskGroupValue, TaskValue, TryRecvResult, Value, VecValue,
};
use crate::diag::Span;
use crate::integer::IntegerValue;
use crate::sema::Type;
use std::thread;

#[test]
fn render_float_formats_current_surface() {
    assert_eq!(render_float(42.0), "42.0");
    assert_eq!(render_float(3.5), "3.5");
    assert_eq!(render_float(f64::INFINITY), "inf");

    let float32_value = (3.14f32) as f64;
    assert_eq!(render_float(float32_value), "3.14");
}

#[test]
fn option_and_result_helpers_render_expected_variants() {
    assert_eq!(
        option_some(Value::Int(IntegerValue::from_signed(7))).render(),
        "Option.Some(7)"
    );
    assert_eq!(option_none().render(), "Option.None");
    assert_eq!(result_ok(Value::Bool(true)).render(), "Result.Ok(true)");
    assert_eq!(
        result_err(Value::String("oops".to_string())).render(),
        "Result.Err(oops)"
    );
    assert_eq!(
        send_error_closed(Value::Int(IntegerValue::from_signed(3))).render(),
        "SendError.Closed(3)"
    );
}

#[test]
fn cast_numeric_value_covers_success_and_failure_paths() {
    assert_eq!(
        cast_numeric_value(
            Value::Int(IntegerValue::from_signed(5)),
            &Type::named("float64"),
            None
        )
        .expect("int to float cast should succeed"),
        Value::Float(5.0)
    );

    assert_eq!(
        cast_numeric_value(Value::Float(3.0), &Type::named("int32"), None)
            .expect("float to int cast should succeed"),
        Value::Int(IntegerValue::from_signed(3))
    );

    let overflow = cast_numeric_value(
        Value::Float(500.0),
        &Type::named("int8"),
        Some(Span::new(4, 9)),
    )
    .expect_err("narrow integer overflow should fail");
    assert!(overflow.message.contains("does not fit in `int8`"));

    let non_numeric = cast_numeric_value(
        Value::String("Aurora".to_string()),
        &Type::named("int32"),
        Some(Span::new(2, 3)),
    )
    .expect_err("non-numeric casts should fail");
    assert!(non_numeric
        .message
        .contains("casts are only supported between numeric types"));

    let float64_precision = cast_numeric_value(
        Value::Int(IntegerValue::from_literal((1u128 << 53) + 1)),
        &Type::named("float64"),
        Some(Span::new(6, 7)),
    )
    .expect_err("precision-losing int64 to float64 casts should fail");
    assert!(float64_precision
        .message
        .contains("cannot be represented exactly as `float64`"));

    let float32_precision = cast_numeric_value(
        Value::Int(IntegerValue::from_literal((1u128 << 24) + 1)),
        &Type::named("float32"),
        Some(Span::new(8, 9)),
    )
    .expect_err("precision-losing int to float32 casts should fail");
    assert!(float32_precision
        .message
        .contains("cannot be represented exactly as `float32`"));
}

#[test]
fn channel_runtime_helpers_cover_send_receive_and_close_paths() {
    let channel = ChannelValue::new();
    assert_eq!(channel.try_recv(), TryRecvResult::Empty);

    channel
        .send(Value::Int(IntegerValue::from_signed(5)))
        .expect("send should succeed on open channel");
    assert_eq!(
        channel.try_recv(),
        TryRecvResult::Value(Value::Int(IntegerValue::from_signed(5)))
    );

    channel.close();
    assert_eq!(channel.try_recv(), TryRecvResult::Closed);
    assert_eq!(
        channel
            .send(Value::Bool(true))
            .expect_err("closed channel should reject sends"),
        Value::Bool(true)
    );
    assert!(channel.recv_blocking().is_none());
}

#[test]
fn task_and_cancellation_helpers_cover_current_runtime_contract() {
    let task = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Int(IntegerValue::from_signed(9)))
    }));
    assert_eq!(
        task.join_result().expect("first join should succeed"),
        Value::Int(IntegerValue::from_signed(9))
    );
    assert_eq!(
        task.join_result().expect("cached join should also succeed"),
        Value::Int(IntegerValue::from_signed(9))
    );

    let cancellation = CancellationContext::default();
    assert!(!cancellation.is_cancelled());
    let group = TaskGroupValue::new(&cancellation);
    let registered = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    group.register_task(registered.clone());
    assert_eq!(group.drain_tasks(), vec![registered]);
    group.cancel();
    assert!(group.child_cancellation().is_cancelled());
}

#[test]
fn channel_and_task_helpers_tolerate_poisoned_locks() {
    let channel = ChannelValue::new();
    let poisoned_channel = channel.clone();
    let _ = thread::spawn(move || {
        let _guard = poisoned_channel
            .inner
            .state
            .lock()
            .expect("poison setup lock");
        panic!("poison channel lock");
    })
    .join();
    channel
        .send(Value::Int(IntegerValue::from_signed(11)))
        .expect("poisoned channel lock should recover");
    assert_eq!(
        channel.try_recv(),
        TryRecvResult::Value(Value::Int(IntegerValue::from_signed(11)))
    );
    channel.close();
    assert_eq!(channel.recv_blocking(), None);

    let cancellation = CancellationContext::default();
    let group = TaskGroupValue::new(&cancellation);
    let poisoned_group = group.clone();
    let _ = thread::spawn(move || {
        let _guard = poisoned_group
            .inner
            .tasks
            .lock()
            .expect("poison setup lock");
        panic!("poison task-group lock");
    })
    .join();
    let registered = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    group.register_task(registered.clone());
    assert_eq!(group.drain_tasks(), vec![registered]);

    let task = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Int(IntegerValue::from_signed(17)))
    }));
    let poisoned_task = task.clone();
    let _ = thread::spawn(move || {
        let _guard = poisoned_task
            .inner
            .handle
            .lock()
            .expect("poison setup lock");
        panic!("poison task lock");
    })
    .join();
    assert_eq!(
        task.join_result()
            .expect("poisoned task handle lock should recover"),
        Value::Int(IntegerValue::from_signed(17))
    );
}

#[test]
fn value_equality_and_render_cover_collection_shapes() {
    let vec_value = Value::Vec(VecValue {
        element_type: Type::named("int32"),
        elements: vec![
            Value::Int(IntegerValue::from_signed(1)),
            Value::Int(IntegerValue::from_signed(2)),
        ],
    });
    assert_eq!(vec_value.render(), "[1, 2]");

    let set_a = Value::Set(SetValue {
        element_type: Type::named("int32"),
        elements: vec![
            Value::Int(IntegerValue::from_signed(1)),
            Value::Int(IntegerValue::from_signed(2)),
        ],
    });
    let set_b = Value::Set(SetValue {
        element_type: Type::named("int32"),
        elements: vec![
            Value::Int(IntegerValue::from_signed(2)),
            Value::Int(IntegerValue::from_signed(1)),
        ],
    });
    assert_eq!(set_a, set_b);

    let map_a = Value::Map(MapValue {
        key_type: Type::named("String"),
        value_type: Type::named("int32"),
        entries: vec![
            (
                Value::String("a".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            ),
            (
                Value::String("b".to_string()),
                Value::Int(IntegerValue::from_signed(2)),
            ),
        ],
    });
    let map_b = Value::Map(MapValue {
        key_type: Type::named("String"),
        value_type: Type::named("int32"),
        entries: vec![
            (
                Value::String("b".to_string()),
                Value::Int(IntegerValue::from_signed(2)),
            ),
            (
                Value::String("a".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            ),
        ],
    });
    assert_eq!(map_a, map_b);

    assert_eq!(
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Done".to_string(),
            payloads: vec![Value::Int(IntegerValue::from_signed(2))],
        })
        .render(),
        "Status.Done(2)"
    );
    assert_eq!(
        Value::Range(RangeValue { start: 1, end: 4 }).render(),
        "range(1, 4)"
    );
}
