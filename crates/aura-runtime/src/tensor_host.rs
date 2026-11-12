//! The host part of the builtin `tensor` module.
//!
//! A `tensor.Node` is a reference-counted handle to memory a device owns.
//! `std/tensor.au` keeps one in each `Tensor[T]` and calls the functions
//! here for every operation. Shapes and dtypes are checked here, on the
//! host, with the Array rules, so a mistake is reported by the call that
//! made it on every device.
//!
//! The CPU device keeps a node's elements in an [`ArrayValue`] and runs
//! each operation through the Array kernels. Integer arithmetic wraps, as
//! it does on a GPU, so a result does not depend on the device.

use super::*;
use host_builtins::HostBuiltinFn;

/// Where a tensor's memory lives. The codes are the ones `std/tensor.au`
/// passes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TensorDevice {
    Cpu = 0,
    Metal = 1,
}

impl TensorDevice {
    pub const fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Cpu),
            1 => Some(Self::Metal),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Metal => "metal",
        }
    }

    /// Whether this process can run work on the device.
    pub fn is_available(self) -> bool {
        matches!(self, Self::Cpu)
    }
}

struct TensorNode {
    device: TensorDevice,
    elements: ArrayValue,
}

/// A `tensor.Node`: shared, immutable device memory.
#[derive(Clone)]
pub struct TensorValue(Arc<TensorNode>);

impl PartialEq for TensorValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for TensorValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<tensor-node>")
    }
}

impl TensorValue {
    fn new(device: TensorDevice, elements: ArrayValue) -> Self {
        Self(Arc::new(TensorNode { device, elements }))
    }

    pub fn device(&self) -> TensorDevice {
        self.0.device
    }

    pub fn dtype(&self) -> ArrayDType {
        self.0.elements.dtype()
    }

    fn elements(&self) -> &ArrayValue {
        &self.0.elements
    }
}

fn tensor_error(name: &str, message: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::coded(
        "AU4007",
        format!(
            "`{}` {message}",
            name.replace("::__aura_", ".").replace("::", ".")
        ),
    )
}

fn node_arg<'a>(args: &'a [Value], index: usize, name: &str) -> Result<&'a TensorValue> {
    match &args[index] {
        Value::Tensor(node) => Ok(node),
        _ => Err(tensor_error(name, "expects a tensor")),
    }
}

fn array_arg<'a>(args: &'a [Value], index: usize, name: &str) -> Result<&'a ArrayValue> {
    match &args[index] {
        Value::Array(array) => Ok(array),
        _ => Err(tensor_error(name, "expects an `Array`")),
    }
}

fn int_arg(args: &[Value], index: usize, name: &str) -> Result<i64> {
    match &args[index] {
        Value::Int(value) => value.as_i128().and_then(|value| i64::try_from(value).ok()),
        _ => None,
    }
    .ok_or_else(|| tensor_error(name, "expects an `int64`"))
}

fn bool_arg(args: &[Value], index: usize, name: &str) -> Result<bool> {
    match &args[index] {
        Value::Bool(value) => Ok(*value),
        _ => Err(tensor_error(name, "expects a `bool`")),
    }
}

fn int_list_arg(args: &[Value], index: usize, name: &str) -> Result<Vec<i64>> {
    let Value::Vec(list) = &args[index] else {
        return Err(tensor_error(name, "expects a `list[int64]`"));
    };
    list.elements
        .iter()
        .map(|element| match &*element {
            Value::Int(value) => value.as_i128().and_then(|value| i64::try_from(value).ok()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| tensor_error(name, "expects a `list[int64]`"))
}

fn device_arg(args: &[Value], index: usize, name: &str) -> Result<TensorDevice> {
    let device = TensorDevice::from_code(int_arg(args, index, name)?)
        .ok_or_else(|| tensor_error(name, "was given an unknown device"))?;
    if device.is_available() {
        Ok(device)
    } else {
        Err(tensor_error(
            name,
            format!("cannot use the {} device on this machine", device.name()),
        ))
    }
}

fn shape_arg(args: &[Value], index: usize, name: &str) -> Result<Box<[usize]>> {
    int_list_arg(args, index, name)?
        .into_iter()
        .map(|dimension| usize::try_from(dimension).ok())
        .collect::<Option<Box<[usize]>>>()
        .ok_or_else(|| tensor_error(name, "expects a shape with no negative dimension"))
}

/// Both operands of a two-tensor operation, which must be on one device.
fn pair_args<'a>(args: &'a [Value], name: &str) -> Result<(&'a TensorValue, &'a TensorValue)> {
    let left = node_arg(args, 0, name)?;
    let right = node_arg(args, 1, name)?;
    if left.device() != right.device() {
        return Err(tensor_error(
            name,
            format!(
                "expects tensors on one device, found {} and {}",
                left.device().name(),
                right.device().name()
            ),
        ));
    }
    Ok((left, right))
}

fn node(device: TensorDevice, elements: ArrayValue) -> Value {
    Value::Tensor(TensorValue::new(device, elements))
}

fn int_value(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

/// A one-element Array holding `value`, the form a whole-tensor reduction
/// gives.
fn single(value: &Value, dtype: ArrayDType) -> Result<ArrayValue> {
    ArrayValue::full(dtype, Box::new([1]), value)
}

/// `tensor::__aura_device_available`.
pub fn host_tensor_device_available(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_device_available";
    host_expect_arity(name, &args, 1)?;
    let device = TensorDevice::from_code(int_arg(&args, 0, name)?);
    Ok(Value::Bool(device.is_some_and(TensorDevice::is_available)))
}

/// `tensor::__aura_from_array`: a tensor holding a copy of an Array's
/// elements.
pub fn host_tensor_from_array(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_from_array";
    host_expect_arity(name, &args, 2)?;
    let device = device_arg(&args, 1, name)?;
    Ok(node(device, array_arg(&args, 0, name)?.detached()?))
}

/// `tensor::__aura_to_array`: an Array holding a copy of a tensor's
/// elements.
pub fn host_tensor_to_array(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_to_array";
    host_expect_arity(name, &args, 1)?;
    Ok(Value::Array(node_arg(&args, 0, name)?.elements().try_clone()?))
}

/// `tensor::__aura_item`: the element of a one-element tensor.
pub fn host_tensor_item(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_item";
    host_expect_arity(name, &args, 1)?;
    let elements = node_arg(&args, 0, name)?.elements();
    if elements.len() != 1 {
        return Err(tensor_error(
            name,
            format!(
                "expects a tensor of one element, found shape {:?}",
                elements.try_shape()?
            ),
        ));
    }
    Ok(elements.value_at_flat(0))
}

/// `tensor::__aura_full`: a tensor of one value.
pub fn host_tensor_full(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_full";
    host_expect_arity(name, &args, 4)?;
    let shape = shape_arg(&args, 0, name)?;
    let dtype = ArrayDType::from_code(int_arg(&args, 2, name)?)
        .ok_or_else(|| tensor_error(name, "was given an unknown dtype"))?;
    let device = device_arg(&args, 3, name)?;
    Ok(node(device, ArrayValue::full(dtype, shape, &args[1])?))
}

/// `tensor::__aura_shape`.
pub fn host_tensor_shape(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_shape";
    host_expect_arity(name, &args, 1)?;
    node_arg(&args, 0, name)?.elements().shape_value()
}

/// `tensor::__aura_len`.
pub fn host_tensor_len(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_len";
    host_expect_arity(name, &args, 1)?;
    let len = node_arg(&args, 0, name)?.elements().len();
    Ok(int_value(
        i64::try_from(len).expect("a live tensor's length fits int64"),
    ))
}

/// `tensor::__aura_device`.
pub fn host_tensor_device(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_device";
    host_expect_arity(name, &args, 1)?;
    Ok(int_value(node_arg(&args, 0, name)?.device() as i64))
}

/// Integer tensors wrap on every device, because a GPU cannot trap. The
/// Array kernels take the wrapping mode for integer elements only.
fn arithmetic_mode(dtype: ArrayDType) -> IntegerArithmeticMode {
    if matches!(dtype, ArrayDType::Int32 | ArrayDType::Int64) {
        IntegerArithmeticMode::Wrapping
    } else {
        IntegerArithmeticMode::Checked
    }
}

/// The first code of the pair operations in `tensor::__aura_binary`. The
/// codes below it are [`ArrayBinaryOp`]'s.
const PAIR_OPERATION_BASE: i64 = 16;

/// `tensor::__aura_binary`: arithmetic, minimum, maximum, or power over
/// two tensors whose shapes broadcast.
pub fn host_tensor_binary(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_binary";
    host_expect_arity(name, &args, 3)?;
    let (left, right) = pair_args(&args, name)?;
    let code = int_arg(&args, 2, name)?;
    let unknown = || tensor_error(name, "was given an unknown operation");
    let elements = if code < PAIR_OPERATION_BASE {
        let operation = ArrayBinaryOp::from_code(code).ok_or_else(unknown)?;
        left.elements().binary(
            right.elements(),
            operation,
            arithmetic_mode(left.dtype()),
        )?
    } else {
        let operation = ArrayPairOp::from_code(code - PAIR_OPERATION_BASE).ok_or_else(unknown)?;
        if operation == ArrayPairOp::Pow && left.dtype() == ArrayDType::Float32 {
            // The Array kernel for `pow` is `float64` only.
            let exponent = right.elements().cast(ArrayDType::Float64)?;
            left.elements()
                .cast(ArrayDType::Float64)?
                .pairwise(&Value::Array(exponent), operation)?
                .cast(ArrayDType::Float32)?
        } else {
            left.elements()
                .pairwise(&Value::Array(right.elements().try_clone()?), operation)?
        }
    };
    Ok(node(left.device(), elements))
}

/// The code of negation in `tensor::__aura_unary`. The codes below it are
/// [`ArrayUnaryOp`]'s.
const NEGATE_OPERATION: i64 = 64;

/// `tensor::__aura_unary`: one function of every element.
pub fn host_tensor_unary(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_unary";
    host_expect_arity(name, &args, 2)?;
    let operand = node_arg(&args, 0, name)?;
    let code = int_arg(&args, 1, name)?;
    let elements = if code == NEGATE_OPERATION {
        ArrayValue::zeros(operand.dtype(), Box::new([1]))?.binary(
            operand.elements(),
            ArrayBinaryOp::Sub,
            arithmetic_mode(operand.dtype()),
        )?
    } else {
        let operation = ArrayUnaryOp::from_code(code)
            .ok_or_else(|| tensor_error(name, "was given an unknown operation"))?;
        let float64_only = !matches!(operation, ArrayUnaryOp::Abs | ArrayUnaryOp::LogicalNot);
        if float64_only && operand.dtype() == ArrayDType::Float32 {
            // The Array kernels for these are `float64` only.
            operand
                .elements()
                .cast(ArrayDType::Float64)?
                .unary(operation)?
                .cast(ArrayDType::Float32)?
        } else {
            operand.elements().unary(operation)?
        }
    };
    Ok(node(operand.device(), elements))
}

/// `tensor::__aura_compare`: a mask of where a comparison holds.
pub fn host_tensor_compare(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_compare";
    host_expect_arity(name, &args, 3)?;
    let (left, right) = pair_args(&args, name)?;
    let operation = ArrayCompareOp::from_code(int_arg(&args, 2, name)?)
        .ok_or_else(|| tensor_error(name, "was given an unknown operation"))?;
    let elements = left
        .elements()
        .compare(&Value::Array(right.elements().try_clone()?), operation)?;
    Ok(node(left.device(), elements))
}

/// `tensor::__aura_where`: each element from the second tensor where the
/// mask holds, else from the third.
pub fn host_tensor_where(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_where";
    host_expect_arity(name, &args, 3)?;
    let (mask, when_true) = pair_args(&args, name)?;
    let (_, when_false) = pair_args(&args[1..], name)?;
    if when_true.dtype() != when_false.dtype() {
        return Err(tensor_error(name, "expects two tensors of one dtype"));
    }
    let elements = mask.elements().choose(
        &Value::Array(when_true.elements().try_clone()?),
        &Value::Array(when_false.elements().try_clone()?),
        when_true.dtype(),
    )?;
    Ok(node(mask.device(), elements))
}

/// `tensor::__aura_reduce`: a reduction over one axis, or over every
/// element when `has_axis` is false, which gives a tensor of shape `[1]`.
/// A mean keeps a float tensor's dtype.
pub fn host_tensor_reduce(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_reduce";
    host_expect_arity(name, &args, 4)?;
    let operand = node_arg(&args, 0, name)?;
    let reduction = ArrayReduction::from_code(int_arg(&args, 1, name)?)
        .ok_or_else(|| tensor_error(name, "was given an unknown reduction"))?;
    let axis = int_arg(&args, 2, name)?;
    let elements = operand.elements();
    let reduced = if bool_arg(&args, 3, name)? {
        elements.reduce_axis(reduction, axis)?
    } else {
        let value = elements.reduce(reduction)?;
        let dtype = match reduction {
            ArrayReduction::Mean | ArrayReduction::FastMean => ArrayDType::Float64,
            _ => operand.dtype(),
        };
        single(&value, dtype)?
    };
    let reduced = if reduced.dtype() != operand.dtype()
        && matches!(operand.dtype(), ArrayDType::Float32 | ArrayDType::Float64) {
        reduced.cast(operand.dtype())?
    } else {
        reduced
    };
    Ok(node(operand.device(), reduced))
}

/// `tensor::__aura_matmul`.
pub fn host_tensor_matmul(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_matmul";
    host_expect_arity(name, &args, 2)?;
    let (left, right) = pair_args(&args, name)?;
    Ok(node(
        left.device(),
        left.elements().matmul(right.elements())?,
    ))
}

/// `tensor::__aura_reshape`.
pub fn host_tensor_reshape(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_reshape";
    host_expect_arity(name, &args, 2)?;
    let operand = node_arg(&args, 0, name)?;
    let shape = int_list_arg(&args, 1, name)?;
    Ok(node(operand.device(), operand.elements().reshape(&shape)?))
}

/// `tensor::__aura_permute`: the axes in a new order. An empty order
/// reverses them.
pub fn host_tensor_permute(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_permute";
    host_expect_arity(name, &args, 2)?;
    let operand = node_arg(&args, 0, name)?;
    let axes = int_list_arg(&args, 1, name)?;
    let elements = if axes.is_empty() {
        operand.elements().transpose()?
    } else {
        operand.elements().permute(&axes)?
    };
    Ok(node(operand.device(), elements))
}

/// `tensor::__aura_cast`: every element converted to another dtype.
pub fn host_tensor_cast(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_cast";
    host_expect_arity(name, &args, 2)?;
    let operand = node_arg(&args, 0, name)?;
    let dtype = ArrayDType::from_code(int_arg(&args, 1, name)?)
        .ok_or_else(|| tensor_error(name, "was given an unknown dtype"))?;
    Ok(node(operand.device(), operand.elements().cast(dtype)?))
}

/// `tensor::__aura_eval`: runs the work a tensor is waiting on. The CPU
/// device runs each operation when it is asked for, so nothing waits.
pub fn host_tensor_eval(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_eval";
    host_expect_arity(name, &args, 1)?;
    node_arg(&args, 0, name)?;
    Ok(Value::Unit)
}

/// `tensor::__aura_share`: another handle to the same memory.
pub fn host_tensor_share(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_share";
    host_expect_arity(name, &args, 1)?;
    Ok(Value::Tensor(node_arg(&args, 0, name)?.clone()))
}

/// The suffixes of the host functions that exist once for each dtype,
/// because their Aura signatures name an `Array[T]` or a `T`.
const DTYPE_SUFFIXES: [&str; 5] = ["_i32", "_i64", "_f32", "_f64", "_bool"];

/// The interpreter's entry for a `tensor` host builtin.
pub fn tensor_host_function(name: &str) -> Option<HostBuiltinFn> {
    let generic = DTYPE_SUFFIXES
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name);
    Some(match generic {
        "tensor::__aura_device_available" => host_tensor_device_available,
        "tensor::__aura_from_array" => host_tensor_from_array,
        "tensor::__aura_to_array" => host_tensor_to_array,
        "tensor::__aura_item" => host_tensor_item,
        "tensor::__aura_full" => host_tensor_full,
        "tensor::__aura_shape" => host_tensor_shape,
        "tensor::__aura_len" => host_tensor_len,
        "tensor::__aura_device" => host_tensor_device,
        "tensor::__aura_binary" => host_tensor_binary,
        "tensor::__aura_unary" => host_tensor_unary,
        "tensor::__aura_compare" => host_tensor_compare,
        "tensor::__aura_where" => host_tensor_where,
        "tensor::__aura_reduce" => host_tensor_reduce,
        "tensor::__aura_matmul" => host_tensor_matmul,
        "tensor::__aura_reshape" => host_tensor_reshape,
        "tensor::__aura_permute" => host_tensor_permute,
        "tensor::__aura_cast" => host_tensor_cast,
        "tensor::__aura_eval" => host_tensor_eval,
        "tensor::__aura_share" => host_tensor_share,
        _ => return None,
    })
}
