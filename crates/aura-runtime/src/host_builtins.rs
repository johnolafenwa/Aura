//! One function per host builtin that takes owned arguments.
//!
//! Native code calls each builtin through its own exported entry, so a program
//! links only the builtins it calls. The MIR interpreter reaches the same
//! functions through `host_builtin_function`.

use super::*;

/// A host builtin evaluated over owned runtime arguments. The second argument
/// carries the program arguments when the caller has them; `None` reads them
/// from the process.
pub type HostBuiltinFn = fn(Vec<Value>, Option<&[String]>) -> Result<Value>;

fn host_math_rounding(name: &str, args: Vec<Value>) -> Result<Value> {
    host_expect_arity(name, &args, 1)?;
    let display_name = name.replace("::", ".");
    let Value::Float(value) = args[0] else {
        return Err(Diagnostic::coded(
            "AU4001",
            format!("`{display_name}` expects `float64`"),
        ));
    };
    let rounded = match name {
        "math::floor" => value.floor(),
        "math::ceil" => value.ceil(),
        "math::trunc" => value.trunc(),
        _ => unreachable!(),
    };
    // `i64::MAX as f64` rounds to 2^63, so the upper bound is
    // intentionally exclusive while -2^63 remains representable.
    const I64_MIN_F64: f64 = -9_223_372_036_854_775_808.0;
    const I64_LIMIT_F64: f64 = 9_223_372_036_854_775_808.0;
    if !rounded.is_finite() || !(I64_MIN_F64..I64_LIMIT_F64).contains(&rounded) {
        return Err(Diagnostic::coded(
            "AU4002",
            format!("`{display_name}` result cannot be represented as `int64`"),
        ));
    }
    let integer = IntegerValue::from_typed_signed(rounded as i64 as i128, IntegerKind::Int64)
        .expect("validated math conversion must fit int64");
    Ok(Value::Int(integer))
}

pub fn host_math_floor(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_math_rounding("math::floor", args)
}

pub fn host_math_ceil(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_math_rounding("math::ceil", args)
}

pub fn host_math_trunc(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_math_rounding("math::trunc", args)
}

pub fn host_math_pow(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "math::pow";
    host_expect_arity(name, &args, 2)?;
    let (Value::Float(base), Value::Float(exponent)) = (&args[0], &args[1]) else {
        return Err(Diagnostic::coded(
            "AU4001",
            "`math.pow` expects two `float64` arguments",
        ));
    };
    float_power(*base, *exponent, FloatPowerWidth::Float64).map(Value::Float)
}

fn host_math_unary(name: &str, args: Vec<Value>) -> Result<Value> {
    host_expect_arity(name, &args, 1)?;
    let display_name = name.replace("::", ".");
    let Value::Float(value) = args[0] else {
        return Err(Diagnostic::coded(
            "AU4001",
            format!("`{display_name}` expects `float64`"),
        ));
    };
    if matches!(name, "math::log" | "math::log2" | "math::log10")
        && value.is_finite()
        && value <= 0.0
    {
        return Err(Diagnostic::coded(
            "AU4001",
            format!("`{display_name}` domain error for `{value}`"),
        ));
    }
    if matches!(name, "math::sin" | "math::cos" | "math::tan") && value.is_infinite() {
        return Err(Diagnostic::coded(
            "AU4001",
            format!("`{display_name}` domain error for `{value}`"),
        ));
    }
    let result = match name {
        "math::exp" => value.exp(),
        "math::log" => value.ln(),
        "math::log2" => value.log2(),
        "math::log10" => value.log10(),
        "math::sin" => value.sin(),
        "math::cos" => value.cos(),
        "math::tan" => value.tan(),
        _ => unreachable!(),
    };
    if name == "math::exp" && value.is_finite() && result.is_infinite() {
        return Err(Diagnostic::coded("AU4002", "`math.exp` overflow"));
    }
    Ok(Value::Float(result))
}

pub fn host_math_exp(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_math_unary("math::exp", args)
}

pub fn host_math_log(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_math_unary("math::log", args)
}

pub fn host_math_log2(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_math_unary("math::log2", args)
}

pub fn host_math_log10(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_math_unary("math::log10", args)
}

pub fn host_math_sin(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_math_unary("math::sin", args)
}

pub fn host_math_cos(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_math_unary("math::cos", args)
}

pub fn host_math_tan(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_math_unary("math::tan", args)
}

pub fn host_control_retry_validate(
    args: Vec<Value>,
    _program_args: Option<&[String]>,
) -> Result<Value> {
    let name = "control::__retry_validate";
    host_expect_arity(name, &args, 2)?;
    let max_attempts = match &args[0] {
        Value::Int(value) => value
            .as_i128()
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| {
                Diagnostic::coded("AU4001", "control.retry max_attempts must be an int32")
            })?,
        _ => {
            return Err(Diagnostic::coded(
                "AU4001",
                "control.retry max_attempts must be an int32",
            ))
        }
    };
    let initial_backoff = match args[1] {
        Value::Duration(value) => value,
        _ => {
            return Err(Diagnostic::coded(
                "AU4001",
                "control.retry initial_backoff must be a Duration",
            ))
        }
    };
    validate_retry_runtime_policy(max_attempts, initial_backoff)?;
    Ok(Value::Unit)
}

pub fn host_control_retry_next_backoff(
    args: Vec<Value>,
    _program_args: Option<&[String]>,
) -> Result<Value> {
    let name = "control::__retry_next_backoff";
    host_expect_arity(name, &args, 1)?;
    let backoff = match args[0] {
        Value::Duration(value) => value,
        _ => {
            return Err(Diagnostic::coded(
                "AU4001",
                "control.retry backoff must be a Duration",
            ))
        }
    };
    next_retry_runtime_backoff(backoff).map(Value::Duration)
}

pub fn host_control_retry_cancel_if_requested(
    args: Vec<Value>,
    _program_args: Option<&[String]>,
) -> Result<Value> {
    let name = "control::__retry_cancel_if_requested";
    host_expect_arity(name, &args, 0)?;
    propagate_retry_task_cancellation();
    Ok(Value::Unit)
}

pub fn host_sys_args(args: Vec<Value>, program_args: Option<&[String]>) -> Result<Value> {
    let name = "sys::args";
    host_expect_arity(name, &args, 0)?;
    Ok(Value::Vec(VecValue {
        element_type: Type::named("str"),
        elements: program_args
            .map(<[String]>::to_vec)
            .unwrap_or_else(host_process_args)
            .into_iter()
            .map(Value::String)
            .collect(),
        str_view: Default::default(),
    }))
}

pub fn host_sys_env(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "sys::env";
    host_expect_arity(name, &args, 1)?;
    let name = host_string_arg(&args, 0, name)?;
    Ok(std::env::var(name)
        .ok()
        .map(Value::String)
        .map(|text| optional_present(aura_core::types::Type::named("str"), text))
        .unwrap_or_else(|| optional_absent(aura_core::types::Type::named("str"))))
}

pub fn host_sys_current_dir(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "sys::current_dir";
    host_expect_arity(name, &args, 0)?;
    Ok(match std::env::current_dir() {
        Ok(path) => result_ok(Value::String(path.to_string_lossy().to_string())),
        Err(error) => result_err(io_error(error)),
    })
}

pub fn host_sys_unix_time_ms(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "sys::unix_time_ms";
    host_expect_arity(name, &args, 0)?;
    let duration = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration,
        Err(error) => {
            return Err(Diagnostic::new(format!(
                "system clock is before unix epoch: {error}"
            )))
        }
    };
    host_millis_value(duration.as_millis(), "unix time")
}

pub fn host_sys_monotonic_time_ms(
    args: Vec<Value>,
    _program_args: Option<&[String]>,
) -> Result<Value> {
    let name = "sys::monotonic_time_ms";
    host_expect_arity(name, &args, 0)?;
    let millis = HOST_MONOTONIC_EPOCH
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis();
    host_millis_value(millis, "monotonic time")
}

pub fn host_path_join(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "path::join";
    host_expect_arity(name, &args, 2)?;
    let base = host_string_arg(&args, 0, name)?;
    let child = host_string_arg(&args, 1, name)?;
    Ok(Value::String(
        Path::new(&base).join(child).to_string_lossy().to_string(),
    ))
}

fn host_path_component(name: &str, args: Vec<Value>) -> Result<Value> {
    host_expect_arity(name, &args, 1)?;
    let path = host_string_arg(&args, 0, name)?;
    let path = Path::new(&path);
    let value = match name {
        "path::parent" => path
            .parent()
            .map(|value| value.to_string_lossy().to_string()),
        "path::file_name" => path
            .file_name()
            .map(|value| value.to_string_lossy().to_string()),
        "path::extension" => path
            .extension()
            .map(|value| value.to_string_lossy().to_string()),
        _ => unreachable!(),
    };
    Ok(value
        .map(Value::String)
        .map(|text| optional_present(aura_core::types::Type::named("str"), text))
        .unwrap_or_else(|| optional_absent(aura_core::types::Type::named("str"))))
}

pub fn host_path_parent(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_path_component("path::parent", args)
}

pub fn host_path_file_name(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_path_component("path::file_name", args)
}

pub fn host_path_extension(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_path_component("path::extension", args)
}

pub fn host_path_is_absolute(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "path::is_absolute";
    host_expect_arity(name, &args, 1)?;
    Ok(Value::Bool(
        Path::new(&host_string_arg(&args, 0, name)?).is_absolute(),
    ))
}

pub fn host_json_parse(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "json::parse";
    host_expect_arity(name, &args, 1)?;
    if let Some(parsed) = json_parse_inline(host_string_ref_arg(&args, 0, name)?) {
        return parsed.into_runtime();
    }
    let (source, reservation) = prepare_json_codec_source(|| {
        clone_json_codec_source(host_string_ref_arg(&args, 0, name)?)
    })?;
    json_parse_owned_to_runtime(source, reservation)
}

pub fn host_json_dumps(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "json::dumps";
    host_expect_arity(name, &args, 2)?;
    let indent = host_json_indent_arg(&args[1])?;
    let value = runtime_value_to_json(&args[0])?;
    json_codec::dumps(&value, indent)
        .map(Value::String)
        .map_err(json_dump_error_to_diagnostic)
}

pub fn host_json_is_null(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "json::is_null";
    host_expect_arity(name, &args, 1)?;
    let variant = host_json_variant(&args[0], name)?;
    if variant.variant_name == "Null" && !variant.payloads.is_empty() {
        return Err(Diagnostic::coded(
            "AU4001",
            "malformed runtime `json.Value.Null` payload in `json::is_null`",
        ));
    }
    Ok(Value::Bool(variant.variant_name == "Null"))
}

pub fn host_json_as_bool(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "json::as_bool";
    host_expect_arity(name, &args, 1)?;
    Ok(match host_json_exact_payload(&args[0], "Bool", name)? {
        Some(Value::Bool(value)) => {
            optional_present(aura_core::types::Type::named("bool"), Value::Bool(value))
        }
        Some(_) => {
            return Err(Diagnostic::coded(
                "AU4001",
                "malformed runtime `json.Value.Bool` payload in `json::as_bool`",
            ))
        }
        None => optional_absent(aura_core::types::Type::named("bool")),
    })
}

pub fn host_json_as_int(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "json::as_int";
    host_expect_arity(name, &args, 1)?;
    Ok(match host_json_exact_payload(&args[0], "Int", name)? {
        Some(Value::Int(value)) if json_int_metadata_is_exact(&value) => {
            optional_present(aura_core::types::Type::named("int64"), Value::Int(value))
        }
        Some(_) => {
            return Err(Diagnostic::coded(
                "AU4001",
                "malformed runtime `json.Value.Int` payload in `json::as_int`",
            ))
        }
        None => optional_absent(aura_core::types::Type::named("int64")),
    })
}

pub fn host_json_as_float(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "json::as_float";
    host_expect_arity(name, &args, 1)?;
    Ok(match host_json_exact_payload(&args[0], "Float", name)? {
        Some(Value::Float(value)) => optional_present(
            aura_core::types::Type::named("float64"),
            Value::Float(value),
        ),
        Some(_) => {
            return Err(Diagnostic::coded(
                "AU4001",
                "malformed runtime `json.Value.Float` payload in `json::as_float`",
            ))
        }
        None => optional_absent(aura_core::types::Type::named("float64")),
    })
}

pub fn host_json_into_string(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "json::into_string";
    host_expect_arity(name, &args, 1)?;
    let value = args
        .into_iter()
        .next()
        .expect("validated host builtin arity provides one argument");
    Ok(match host_json_into_exact_payload(value, "String", name)? {
        Some(Value::String(value)) => {
            optional_present(json_into_member_type("String"), Value::String(value))
        }
        Some(_) => {
            return Err(Diagnostic::coded(
                "AU4001",
                "malformed runtime `json.Value.String` payload in `json::into_string`",
            ))
        }
        None => optional_absent(json_into_member_type("String")),
    })
}

pub fn host_json_into_array(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "json::into_array";
    host_expect_arity(name, &args, 1)?;
    let value = args
        .into_iter()
        .next()
        .expect("validated host builtin arity provides one argument");
    Ok(match host_json_into_exact_payload(value, "Array", name)? {
        Some(Value::Vec(value)) if json_array_metadata_is_exact(&value) => {
            optional_present(json_into_member_type("Array"), Value::Vec(value))
        }
        Some(_) => {
            return Err(Diagnostic::coded(
                "AU4001",
                "malformed runtime `json.Value.Array` payload in `json::into_array`",
            ))
        }
        None => optional_absent(json_into_member_type("Array")),
    })
}

pub fn host_json_into_object(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "json::into_object";
    host_expect_arity(name, &args, 1)?;
    let value = args
        .into_iter()
        .next()
        .expect("validated host builtin arity provides one argument");
    Ok(match host_json_into_exact_payload(value, "Object", name)? {
        Some(Value::Map(value)) if json_object_metadata_is_exact(&value) => {
            optional_present(json_into_member_type("Object"), Value::Map(value))
        }
        Some(_) => {
            return Err(Diagnostic::coded(
                "AU4001",
                "malformed runtime `json.Value.Object` payload in `json::into_object`",
            ))
        }
        None => optional_absent(json_into_member_type("Object")),
    })
}

pub fn host_json_is_valid(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "json::is_valid";
    host_expect_arity(name, &args, 1)?;
    Ok(Value::Bool(legacy_json_is_valid(&host_string_arg(
        &args, 0, name,
    )?)))
}

pub fn host_json_stringify_map(
    args: Vec<Value>,
    _program_args: Option<&[String]>,
) -> Result<Value> {
    let name = "json::stringify_map";
    host_expect_arity(name, &args, 1)?;
    let value = host_string_map_arg(&args, 0, name)?;
    Ok(match serde_json::to_string(&value) {
        Ok(text) => result_ok(Value::String(text)),
        Err(error) => result_err(Value::String(error.to_string())),
    })
}

pub fn host_json_parse_string_map(
    args: Vec<Value>,
    _program_args: Option<&[String]>,
) -> Result<Value> {
    let name = "json::parse_string_map";
    host_expect_arity(name, &args, 1)?;
    let text = host_string_arg(&args, 0, name)?;
    Ok(
        match serde_json::from_str::<BTreeMap<String, String>>(&text) {
            Ok(value) => result_ok(host_string_map_value(value)),
            Err(error) => result_err(Value::String(error.to_string())),
        },
    )
}

pub fn host_toml_is_valid(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "toml::is_valid";
    host_expect_arity(name, &args, 1)?;
    Ok(Value::Bool(
        toml::from_str::<toml::Value>(&host_string_arg(&args, 0, name)?).is_ok(),
    ))
}

pub fn host_toml_stringify_map(
    args: Vec<Value>,
    _program_args: Option<&[String]>,
) -> Result<Value> {
    let name = "toml::stringify_map";
    host_expect_arity(name, &args, 1)?;
    let value = host_string_map_arg(&args, 0, name)?;
    Ok(match toml::to_string(&value) {
        Ok(text) => result_ok(Value::String(text)),
        Err(error) => result_err(Value::String(error.to_string())),
    })
}

pub fn host_toml_parse_string_map(
    args: Vec<Value>,
    _program_args: Option<&[String]>,
) -> Result<Value> {
    let name = "toml::parse_string_map";
    host_expect_arity(name, &args, 1)?;
    let text = host_string_arg(&args, 0, name)?;
    Ok(match toml::from_str::<BTreeMap<String, String>>(&text) {
        Ok(value) => result_ok(host_string_map_value(value)),
        Err(error) => result_err(Value::String(error.to_string())),
    })
}

pub fn host_metrics_increment(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "metrics::increment";
    host_expect_arity(name, &args, 2)?;
    let metric = host_string_arg(&args, 0, name)?;
    let Some(Value::Int(value)) = args.get(1) else {
        return Err(Diagnostic::new(
            "`metrics.increment` expects `int64` for `value`",
        ));
    };
    let value = value
        .as_i128()
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| Diagnostic::new("metric increment does not fit in `int64`"))?;
    let mut metrics = lock_mutex(HOST_METRICS.get_or_init(|| Mutex::new(BTreeMap::new())));
    let entry = metrics.entry(metric).or_insert(0);
    *entry = entry
        .checked_add(value)
        .ok_or_else(|| Diagnostic::new("metric value overflowed `int64`"))?;
    Ok(Value::Unit)
}

pub fn host_metrics_get(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "metrics::get";
    host_expect_arity(name, &args, 1)?;
    let metric = host_string_arg(&args, 0, name)?;
    let metrics = lock_mutex(HOST_METRICS.get_or_init(|| Mutex::new(BTreeMap::new())));
    Ok(Value::Int(IntegerValue::from_signed(i128::from(
        metrics.get(&metric).copied().unwrap_or(0),
    ))))
}

pub fn host_metrics_reset(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "metrics::reset";
    host_expect_arity(name, &args, 0)?;
    lock_mutex(HOST_METRICS.get_or_init(|| Mutex::new(BTreeMap::new()))).clear();
    Ok(Value::Unit)
}

fn host_log_record(name: &str, args: Vec<Value>) -> Result<Value> {
    host_expect_arity(name, &args, 2)?;
    let message = host_string_arg(&args, 0, name)?;
    let fields = host_string_map_arg(&args, 1, name)?;
    let record = serde_json::json!({
        "kind": if name == "trace::event" { "trace" } else { "log" },
        "level": name.split_once("::").map(|(_, value)| value).unwrap_or(name),
        "message": message,
        "fields": fields,
    });
    eprintln!("{record}");
    Ok(Value::Unit)
}

pub fn host_log_debug(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_log_record("log::debug", args)
}

pub fn host_log_info(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_log_record("log::info", args)
}

pub fn host_log_warn(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_log_record("log::warn", args)
}

pub fn host_log_error(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_log_record("log::error", args)
}

pub fn host_trace_event(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_log_record("trace::event", args)
}

/// The owned-argument function for a host builtin, or `None` for a name the
/// table does not hold.
pub fn host_builtin_function(name: &str) -> Option<HostBuiltinFn> {
    let function: HostBuiltinFn = match name {
        "math::floor" => host_math_floor,
        "math::ceil" => host_math_ceil,
        "math::trunc" => host_math_trunc,
        "math::pow" => host_math_pow,
        "math::exp" => host_math_exp,
        "math::log" => host_math_log,
        "math::log2" => host_math_log2,
        "math::log10" => host_math_log10,
        "math::sin" => host_math_sin,
        "math::cos" => host_math_cos,
        "math::tan" => host_math_tan,
        "control::__retry_validate" => host_control_retry_validate,
        "control::__retry_next_backoff" => host_control_retry_next_backoff,
        "control::__retry_cancel_if_requested" => host_control_retry_cancel_if_requested,
        "sys::args" => host_sys_args,
        "sys::env" => host_sys_env,
        "sys::current_dir" => host_sys_current_dir,
        "sys::unix_time_ms" => host_sys_unix_time_ms,
        "sys::monotonic_time_ms" => host_sys_monotonic_time_ms,
        "path::join" => host_path_join,
        "path::parent" => host_path_parent,
        "path::file_name" => host_path_file_name,
        "path::extension" => host_path_extension,
        "path::is_absolute" => host_path_is_absolute,
        "json::parse" => host_json_parse,
        "json::dumps" => host_json_dumps,
        "json::is_null" => host_json_is_null,
        "json::as_bool" => host_json_as_bool,
        "json::as_int" => host_json_as_int,
        "json::as_float" => host_json_as_float,
        "json::into_string" => host_json_into_string,
        "json::into_array" => host_json_into_array,
        "json::into_object" => host_json_into_object,
        "json::is_valid" => host_json_is_valid,
        "json::stringify_map" => host_json_stringify_map,
        "json::parse_string_map" => host_json_parse_string_map,
        "toml::is_valid" => host_toml_is_valid,
        "toml::stringify_map" => host_toml_stringify_map,
        "toml::parse_string_map" => host_toml_parse_string_map,
        "metrics::increment" => host_metrics_increment,
        "metrics::get" => host_metrics_get,
        "metrics::reset" => host_metrics_reset,
        "log::debug" => host_log_debug,
        "log::info" => host_log_info,
        "log::warn" => host_log_warn,
        "log::error" => host_log_error,
        "trace::event" => host_trace_event,
        _ => return None,
    };
    Some(function)
}

pub fn host_bytes_hex_encode_ref(value: &Value) -> Result<Value> {
    let name = "bytes::hex_encode";
    preflight_runtime_bytes_encoder(value, name, bytes_codec::hex_encoded_len)
        .and_then(host_bytes_from_runtime_elements)
        .and_then(|bytes| {
            bytes_resource_only(bytes_codec::hex_encode(&bytes), name).map(Value::String)
        })
}

pub fn host_bytes_hex_decode_ref(value: &Value) -> Result<Value> {
    let text = host_string_value_ref(value, 0, "bytes::hex_decode")?;
    match bytes_codec::hex_decode(text) {
        Ok(bytes) => {
            runtime_bytes_from_host(&bytes).and_then(|value| bytes_runtime_result("Ok", value))
        }
        Err(error) => bytes_codec_error_to_result(error),
    }
}

pub fn host_bytes_base64_encode_ref(value: &Value) -> Result<Value> {
    let name = "bytes::base64_encode";
    preflight_runtime_bytes_encoder(value, name, bytes_codec::base64_encoded_len)
        .and_then(host_bytes_from_runtime_elements)
        .and_then(|bytes| {
            bytes_resource_only(bytes_codec::base64_encode(&bytes), name).map(Value::String)
        })
}

pub fn host_bytes_base64_decode_ref(value: &Value) -> Result<Value> {
    let text = host_string_value_ref(value, 0, "bytes::base64_decode")?;
    match bytes_codec::base64_decode(text) {
        Ok(bytes) => {
            runtime_bytes_from_host(&bytes).and_then(|value| bytes_runtime_result("Ok", value))
        }
        Err(error) => bytes_codec_error_to_result(error),
    }
}

pub fn host_bytes_sha256_ref(value: &Value) -> Result<Value> {
    let name = "bytes::sha256";
    host_bytes_from_runtime(value, name).and_then(|bytes| {
        bytes_resource_only(bytes_codec::sha256_bytes(&bytes), name)
            .and_then(|digest| runtime_bytes_from_host(&digest))
    })
}

pub fn host_bytes_sha256_string_ref(value: &Value) -> Result<Value> {
    let name = "bytes::sha256_string";
    let text = host_string_value_ref(value, 0, name)?;
    bytes_resource_only(bytes_codec::sha256_string(text), name)
        .and_then(|digest| runtime_bytes_from_host(&digest))
}

pub fn host_str_to_bytes_ref(value: &Value) -> Result<Value> {
    evaluate_string_to_bytes_host_ref(host_string_value_ref(value, 0, "str.to_bytes")?)
}

pub fn host_str_from_bytes_ref(value: &Value) -> Result<Value> {
    let name = "str.from_bytes";
    let elements = runtime_uint8_elements(value, name)?;
    let error_index = match elements.as_uint8() {
        Some(bytes) => runtime_utf8_error_index(bytes.iter().copied()),
        None => runtime_utf8_error_index(
            elements
                .iter()
                .map(|value| exact_runtime_uint8(&value).expect("runtime bytes were validated")),
        ),
    };
    if let Some(index) = error_index {
        return bytes_codec_error_to_result(BytesCodecError::Data(BytesDataError::InvalidUtf8 {
            index,
        }));
    }
    let bytes = host_bytes_from_runtime_elements(elements)?;
    match bytes_codec::string_from_bytes(&bytes) {
        Ok(text) => bytes_runtime_result("Ok", Value::String(text)),
        Err(error) => bytes_codec_error_to_result(error),
    }
}
