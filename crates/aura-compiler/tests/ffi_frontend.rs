use aura_compiler::ast::{ExternFunctionDecl, ExternOpaqueClassDecl, Item, TypeRefKind};
use aura_compiler::ffi::{
    call_host_function, call_process_symbol, FfiError, FfiSignature, FfiType, FfiValue,
    HostFunction, OpaqueHandle,
};
use aura_compiler::lexer::{lex, TokenKind};
use aura_compiler::{analyze_path_source, analyze_source, complete_path_source, parse_source};
use std::ffi::c_void;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let unique = format!(
            "{prefix}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after the Unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("temporary package should be created");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

unsafe extern "C" fn observe_public_ffi_contract(
    boolean: u8,
    i8_value: i8,
    i16_value: i16,
    i32_value: i32,
    i64_value: i64,
    u8_value: u8,
    u16_value: u16,
    u32_value: u32,
    u64_value: u64,
    f32_value: f32,
    f64_value: f64,
    text: *const u8,
    text_len: usize,
    bytes: *const u8,
    bytes_len: usize,
    mutable_bytes: *mut u8,
    mutable_bytes_len: usize,
    handle: *mut c_void,
) -> i64 {
    let text = unsafe { std::slice::from_raw_parts(text, text_len) };
    let bytes = unsafe { std::slice::from_raw_parts(bytes, bytes_len) };
    let mutable_bytes = unsafe { std::slice::from_raw_parts_mut(mutable_bytes, mutable_bytes_len) };
    let all_values_arrived = boolean == 1
        && i8_value == -8
        && i16_value == -1_600
        && i32_value == -32_000
        && i64_value == -64_000
        && u8_value == 8
        && u16_value == 1_600
        && u32_value == 32_000
        && u64_value == 64_000
        && f32_value == 3.5
        && f64_value == 7.25
        && text == "snowman ☃".as_bytes()
        && bytes == [1, 2, 3]
        && !handle.is_null();
    if all_values_arrived {
        for byte in mutable_bytes {
            *byte = byte.wrapping_add(1);
        }
        42
    } else {
        -1
    }
}

unsafe extern "C" fn return_invalid_bool() -> u8 {
    2
}

unsafe extern "C" fn return_null_handle() -> *mut c_void {
    std::ptr::null_mut()
}

fn host_function(function: *const ()) -> HostFunction {
    HostFunction::new(function.cast_mut().cast()).expect("test function pointer should be non-null")
}

#[test]
fn extern_c_declarations_preserve_their_bodyless_public_ast_contract() {
    let module = parse_source(concat!(
        "public extern \"C\" opaque class ProcessHandle\n",
        "public extern \"C\" def getpid() -> int32\n",
        "extern \"C\" def write(fd: int32, bytes: list[uint8]) -> int64\n",
    ))
    .expect("FFI declarations should parse");

    assert!(matches!(
        &module.items[0],
        Item::ExternOpaqueClass(ExternOpaqueClassDecl {
            public: true,
            abi,
            name,
            ..
        }) if abi == "C" && name == "ProcessHandle"
    ));
    assert!(matches!(
        &module.items[1],
        Item::ExternFunction(ExternFunctionDecl {
            public: true,
            abi,
            name,
            params,
            return_type,
            ..
        }) if abi == "C"
            && name == "getpid"
            && params.is_empty()
            && matches!(
                &return_type.kind,
                TypeRefKind::Named { name, args } if name == "int32" && args.is_empty()
            )
    ));
    assert!(matches!(
        &module.items[2],
        Item::ExternFunction(ExternFunctionDecl {
            public: false,
            name,
            params,
            ..
        }) if name == "write" && params.len() == 2
    ));

    let serialized =
        serde_json::to_value(&module.items[1]).expect("extern function AST should serialize");
    assert_eq!(serialized["ExternFunction"]["abi"], "C");
    assert_eq!(serialized["ExternFunction"]["name"], "getpid");
    assert!(serialized["ExternFunction"].get("body").is_none());

    let analysis = analyze_source(
        "public extern \"C\" opaque class ProcessHandle\npublic extern \"C\" def getpid() -> int32\n",
    );
    assert!(analysis.symbols.iter().any(|symbol| {
        symbol.name == "ProcessHandle"
            && symbol.kind == "class"
            && symbol.detail == "extern \"C\" opaque"
    }));
    assert!(analysis.symbols.iter().any(|symbol| {
        symbol.name == "getpid"
            && symbol.kind == "function"
            && symbol.detail == "extern \"C\" -> int32"
    }));
}

#[test]
fn ffi_keywords_and_reserved_syntax_have_stable_frontend_diagnostics() {
    let tokens = lex("extern opaque\n").expect("FFI keywords should lex");
    assert!(tokens.iter().any(|token| token.kind == TokenKind::KwExtern));
    assert!(tokens.iter().any(|token| token.kind == TokenKind::KwOpaque));

    for (source, expected) in [
        (
            "extern C def local() -> int32\n",
            "expected ABI string `\"C\"` after `extern`",
        ),
        (
            "extern \"Rust\" def local() -> int32\n",
            "FFI v0 supports only `extern \"C\"` declarations",
        ),
        (
            "extern \"C\" def getpid() -> int32:\n    return 1\n",
            "`extern` function declarations have no Aura body; remove `:` and the indented block",
        ),
        (
            "extern \"C\" def flush()\n",
            "`extern` function declarations require an explicit return type; write `-> None` when the function returns no value",
        ),
        (
            "extern \"C\" def write(fd: int32 = 1) -> int64\n",
            "`extern` function parameters cannot have default values",
        ),
        (
            "extern \"C\" def identity[T](value: T) -> T\n",
            "FFI v0 `extern` declarations cannot have type parameters",
        ),
        (
            "extern \"C\" opaque class Handle[T]\n",
            "FFI v0 opaque handle declarations cannot have type parameters",
        ),
        (
            "extern \"C\" def printf(format: str, ...) -> int32\n",
            "FFI v0 does not support variadic declarations; declare fixed parameters explicitly",
        ),
        (
            "extern \"C\" def visit(callback: def(int32) -> None) -> int32\n",
            "FFI v0 does not support callback parameters or returns",
        ),
        (
            "extern \"C\" def read(output: *uint8) -> int64\n",
            "FFI v0 does not expose raw pointer syntax; use a supported byte/string view or opaque handle",
        ),
    ] {
        let error = parse_source(source).expect_err("reserved FFI syntax should be rejected");
        assert_eq!(error.code, "AU1101", "{source}");
        assert_eq!(error.message, expected, "{source}");
    }
}

#[test]
fn opted_in_package_analysis_exposes_ffi_completions_hovers_and_definitions() {
    let temp_dir = TempDir::new("aura-ffi-public-analysis");
    let source_dir = temp_dir.path.join("src");
    fs::create_dir_all(&source_dir).expect("package source directory should be created");
    fs::write(
        temp_dir.path.join("Aura.toml"),
        concat!(
            "[package]\n",
            "name = \"ffi_public_analysis\"\n",
            "version = \"0.1.0\"\n",
            "edition = \"2026\"\n",
            "allow_ffi = true\n",
        ),
    )
    .expect("FFI package manifest should be written");

    let native_path = source_dir.join("native.au");
    fs::write(
        &native_path,
        concat!(
            "public extern \"C\" opaque class Handle\n",
            "public extern \"C\" def acquire() -> Handle\n",
            "public extern \"C\" def release(handle: own Handle) -> int32\n",
        ),
    )
    .expect("FFI declarations should be written");

    let main_path = source_dir.join("main.au");
    let source = concat!(
        "import native\n",
        "\n",
        "def main() -> int32:\n",
        "    handle = native.acquire()\n",
        "    return native.release(handle)\n",
    );
    fs::write(&main_path, source).expect("importing source should be written");

    let completion_source = concat!(
        "import native\n",
        "\n",
        "def main() -> int32:\n",
        "    native.\n",
        "    return 0\n",
    );
    let completions = complete_path_source(&main_path, completion_source, 3, 11, Some('.'))
        .expect("member completion should understand the imported FFI module");
    for (name, kind, detail) in [
        ("Handle", "class", "extern \"C\" opaque class"),
        ("acquire", "function", "extern \"C\" acquire() -> Handle"),
        (
            "release",
            "function",
            "extern \"C\" release(handle: own Handle) -> int32",
        ),
    ] {
        assert!(
            completions.iter().any(|completion| {
                completion.name == name && completion.kind == kind && completion.detail == detail
            }),
            "missing completion {name}: {completions:?}"
        );
    }

    let analysis = analyze_path_source(&main_path, source);
    assert!(
        analysis.diagnostics.is_empty(),
        "valid opted-in FFI source should analyze cleanly: {:?}",
        analysis.diagnostics
    );
    let canonical_native_path = fs::canonicalize(&native_path)
        .expect("native module should canonicalize")
        .display()
        .to_string();
    for (hover, line, start, end) in [
        ("extern \"C\" function acquire() -> Handle", 1, 22, 29),
        (
            "extern \"C\" function release(handle: own Handle) -> int32",
            2,
            22,
            29,
        ),
    ] {
        let occurrence = analysis
            .occurrences
            .iter()
            .find(|occurrence| occurrence.hover.contains(hover))
            .unwrap_or_else(|| panic!("missing FFI hover `{hover}`: {:?}", analysis.occurrences));
        assert_eq!(
            occurrence.definition.as_ref(),
            Some(&aura_compiler::analysis::AnalysisRange {
                file_path: Some(canonical_native_path.clone()),
                line,
                start_character: start,
                end_character: end,
            })
        );
    }
}

#[test]
fn public_ffi_engine_preserves_values_views_handles_and_signature_metadata() {
    let mut payload = Box::new(27_u64);
    let payload_pointer = (&mut *payload as *mut u64).cast::<c_void>();
    let handle = OpaqueHandle::new(payload_pointer).expect("box pointer should be non-null");
    assert_eq!(handle.as_ptr(), payload_pointer);

    let parameter_types = vec![
        FfiType::Bool,
        FfiType::I8,
        FfiType::I16,
        FfiType::I32,
        FfiType::I64,
        FfiType::U8,
        FfiType::U16,
        FfiType::U32,
        FfiType::U64,
        FfiType::F32,
        FfiType::F64,
        FfiType::StringView,
        FfiType::BytesView,
        FfiType::BytesViewMut,
        FfiType::OpaqueHandle,
    ];
    let signature = FfiSignature::new(parameter_types.clone(), FfiType::I64);
    assert_eq!(signature.parameters(), parameter_types);
    assert_eq!(signature.result(), FfiType::I64);
    assert_eq!(
        parameter_types
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [
            "bool",
            "int8",
            "int16",
            "int32",
            "int64",
            "uint8",
            "uint16",
            "uint32",
            "uint64",
            "float32",
            "float64",
            "str view",
            "list[uint8] view",
            "mut list[uint8] view",
            "opaque handle",
        ]
    );

    let mut arguments = vec![
        FfiValue::Bool(true),
        FfiValue::I8(-8),
        FfiValue::I16(-1_600),
        FfiValue::I32(-32_000),
        FfiValue::I64(-64_000),
        FfiValue::U8(8),
        FfiValue::U16(1_600),
        FfiValue::U32(32_000),
        FfiValue::U64(64_000),
        FfiValue::F32(3.5),
        FfiValue::F64(7.25),
        FfiValue::String("snowman ☃".to_owned()),
        FfiValue::Bytes(vec![1, 2, 3]),
        FfiValue::Bytes(vec![10, 20, 30]),
        FfiValue::OpaqueHandle(handle),
    ];
    assert_eq!(
        arguments.iter().map(FfiValue::ffi_type).collect::<Vec<_>>(),
        [
            FfiType::Bool,
            FfiType::I8,
            FfiType::I16,
            FfiType::I32,
            FfiType::I64,
            FfiType::U8,
            FfiType::U16,
            FfiType::U32,
            FfiType::U64,
            FfiType::F32,
            FfiType::F64,
            FfiType::StringView,
            FfiType::BytesView,
            FfiType::BytesView,
            FfiType::OpaqueHandle,
        ]
    );

    // SAFETY: the test function has exactly the flattened signature described
    // above and does not retain any pointer passed to it.
    let result = unsafe {
        call_host_function(
            host_function(observe_public_ffi_contract as *const ()),
            &signature,
            &mut arguments,
        )
    };
    assert_eq!(result, Ok(FfiValue::I64(42)));
    assert_eq!(arguments[13], FfiValue::Bytes(vec![11, 21, 31]));
}

#[test]
fn public_ffi_engine_reports_boundary_failures_without_invoking_invalid_calls() {
    let identity_signature = FfiSignature::new(vec![FfiType::I32], FfiType::I32);
    // SAFETY: arity validation rejects this call before invoking the supplied
    // function pointer.
    let arity_error = unsafe {
        call_host_function(
            host_function(observe_public_ffi_contract as *const ()),
            &identity_signature,
            &mut [],
        )
    }
    .expect_err("missing arguments should be rejected");
    assert_eq!(
        arity_error,
        FfiError::ArityMismatch {
            expected: 1,
            actual: 0,
        }
    );
    assert_eq!(
        arity_error.to_string(),
        "FFI call expected 1 argument(s), but received 0"
    );

    // SAFETY: type validation rejects both calls before invoking the supplied
    // function pointer.
    let type_mismatch = unsafe {
        call_host_function(
            host_function(observe_public_ffi_contract as *const ()),
            &identity_signature,
            &mut [FfiValue::Bool(true)],
        )
    }
    .expect_err("a bool must not cross an int32 ABI parameter");
    assert_eq!(
        type_mismatch,
        FfiError::ArgumentTypeMismatch {
            index: 0,
            expected: FfiType::I32,
            actual: FfiType::Bool,
        }
    );
    assert_eq!(
        type_mismatch.to_string(),
        "FFI argument 1 expected int32, but received bool"
    );

    let unsupported_unit = unsafe {
        call_host_function(
            host_function(observe_public_ffi_contract as *const ()),
            &FfiSignature::new(vec![FfiType::Unit], FfiType::Unit),
            &mut [FfiValue::Unit],
        )
    }
    .expect_err("None is not a representable C parameter");
    assert_eq!(
        unsupported_unit,
        FfiError::UnsupportedArgumentType {
            index: 0,
            ffi_type: FfiType::Unit,
        }
    );
    assert_eq!(
        unsupported_unit.to_string(),
        "FFI argument 1 cannot use None at the C boundary"
    );

    for view_type in [
        FfiType::StringView,
        FfiType::BytesView,
        FfiType::BytesViewMut,
    ] {
        // SAFETY: return-type validation rejects the call before invocation.
        let error = unsafe {
            call_host_function(
                host_function(observe_public_ffi_contract as *const ()),
                &FfiSignature::new(Vec::new(), view_type),
                &mut [],
            )
        }
        .expect_err("borrowed views cannot be FFI results");
        assert_eq!(error, FfiError::UnsupportedReturnType(view_type));
        assert_eq!(
            error.to_string(),
            format!("FFI functions cannot return {view_type}")
        );
    }

    // SAFETY: the functions have exactly the declared zero-argument C
    // signatures and do not unwind.
    let invalid_bool = unsafe {
        call_host_function(
            host_function(return_invalid_bool as *const ()),
            &FfiSignature::new(Vec::new(), FfiType::Bool),
            &mut [],
        )
    }
    .expect_err("noncanonical C bool values should be rejected");
    assert_eq!(invalid_bool, FfiError::NonCanonicalBoolReturn(2));
    assert_eq!(
        invalid_bool.to_string(),
        "FFI bool return must be encoded as 0 or 1, but received 2"
    );

    // SAFETY: the function has the exact declared zero-argument pointer result
    // signature and returns normally.
    let null_handle = unsafe {
        call_host_function(
            host_function(return_null_handle as *const ()),
            &FfiSignature::new(Vec::new(), FfiType::OpaqueHandle),
            &mut [],
        )
    }
    .expect_err("null opaque handles should be rejected");
    assert_eq!(null_handle, FfiError::NullOpaqueHandleReturn);
    assert_eq!(
        null_handle.to_string(),
        "FFI function returned a null opaque handle"
    );
    assert!(OpaqueHandle::new(std::ptr::null_mut()).is_none());
    assert!(HostFunction::new(std::ptr::null_mut()).is_none());
}

#[cfg(unix)]
#[test]
fn process_symbol_api_resolves_system_functions_and_teaches_lookup_failures() {
    let signature = FfiSignature::new(Vec::new(), FfiType::I32);
    // SAFETY: POSIX getpid has the exact `int getpid(void)` signature.
    let process_id = unsafe { call_process_symbol("getpid", &signature, &mut []) }
        .expect("getpid should resolve");
    let FfiValue::I32(process_id) = process_id else {
        panic!("getpid should return int32");
    };
    assert!(process_id > 0);

    // SAFETY: neither failure reaches a foreign function call.
    let missing =
        unsafe { call_process_symbol("__aura_missing_public_ffi_symbol__", &signature, &mut []) }
            .expect_err("missing symbols should fail safely");
    assert!(matches!(
        &missing,
        FfiError::SymbolNotFound { symbol, detail }
            if symbol == "__aura_missing_public_ffi_symbol__" && !detail.is_empty()
    ));
    assert!(missing
        .to_string()
        .starts_with("FFI symbol `__aura_missing_public_ffi_symbol__` was not found:"));

    let invalid = unsafe { call_process_symbol("getpid\0ignored", &signature, &mut []) }
        .expect_err("interior NUL should be rejected before symbol lookup");
    assert_eq!(invalid, FfiError::InvalidSymbolName);
    assert_eq!(
        invalid.to_string(),
        "FFI symbol names cannot contain a NUL byte"
    );
}

fn write_checked_ffi_package(temp: &TempDir, source: &str, allow_ffi: bool) -> PathBuf {
    let source_dir = temp.path.join("src");
    fs::create_dir_all(&source_dir).expect("package source directory should be created");
    fs::write(
        temp.path.join("Aura.toml"),
        format!(
            "[package]\nname = \"ffi_contract\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = {allow_ffi}\n"
        ),
    )
    .expect("package manifest should be written");
    let main_path = source_dir.join("main.au");
    fs::write(&main_path, source).expect("package entry source should be written");
    main_path
}

#[test]
fn public_path_check_preserves_canonical_imported_opaque_handle_identity() {
    let temp = TempDir::new("aura-ffi-canonical-handle");
    let main_path = write_checked_ffi_package(
        &temp,
        r#"
import native

extern "C" def inspect(handle: native.Handle) -> native.Handle

def main():
    handle = native.acquire()
    inspected: native.Handle = inspect(handle)
"#,
        true,
    );
    fs::write(
        temp.path.join("src/native.au"),
        concat!(
            "public extern \"C\" opaque class Handle\n",
            "public extern \"C\" def acquire() -> Handle\n",
        ),
    )
    .expect("native module should be written");

    let program = aura_compiler::check_path(&main_path)
        .expect("qualified public opaque handles should keep one canonical nominal identity");
    let signature = &program.extern_functions["inspect"].signature;
    assert_eq!(signature.params[0].to_string(), "native.Handle");
    assert_eq!(signature.return_type.to_string(), "native.Handle");
    assert!(
        !signature.params[0].is_copy(),
        "an opaque foreign handle must not silently acquire Copy ownership"
    );
}

#[test]
fn public_path_check_pins_extern_results_and_reachable_aggregate_rejections() {
    for (case, source, expected) in [
        (
            "declared result",
            r#"
extern "C" def current_value() -> int64

def main():
    narrowed: int32 = current_value()
"#,
            "result type mismatch for extern function `current_value`: expected `int64`, found `int32`",
        ),
        (
            "tuple parameter",
            "extern \"C\" def bad(value: (int32, int32)) -> None\n",
            "FFI v0 does not support parameter type `(int32, int32)`",
        ),
        (
            "tuple result",
            "extern \"C\" def bad() -> (int32, int32)\n",
            "FFI v0 does not support return type `(int32, int32)`",
        ),
    ] {
        let temp = TempDir::new(&format!("aura-ffi-{case}"));
        let main_path = write_checked_ffi_package(&temp, source, true);
        let diagnostic = aura_compiler::check_path(&main_path)
            .expect_err("the public path checker must enforce the FFI signature contract");
        assert_eq!(diagnostic.code, "AU2002", "{case}: {diagnostic:?}");
        assert_eq!(diagnostic.message, expected, "{case}");
    }
}

#[test]
fn public_path_check_preserves_opaque_and_callable_equality_diagnostics() {
    for (case, source, expected_code, expected_message, expected_help) in [
        (
            "right operand",
            r#"
extern "C" opaque class Handle
extern "C" def acquire() -> Handle

def main():
    handle = acquire()
    result = 0 != handle
"#,
            "AU2008",
            "cannot compare `Handle` because opaque FFI handle `Handle` does not define equality",
            vec![
                "compare a stable scalar or str identifier exposed by the binding instead of foreign identity"
                    .to_string(),
            ],
        ),
        (
            "closure capture",
            r#"
extern "C" opaque class Handle
extern "C" def acquire() -> Handle

def main():
    left_handle = acquire()
    right_handle = acquire()
    left: def() -> Handle = lambda: left_handle
    right: def() -> Handle = lambda: right_handle
    result = left == right
"#,
            "AU2008",
            "callable equality is not supported; compare results or use an explicit discriminant",
            Vec::new(),
        ),
    ] {
        let temp = TempDir::new(&format!("aura-ffi-{case}"));
        let main_path = write_checked_ffi_package(&temp, source, true);
        let diagnostic = aura_compiler::check_path(&main_path)
            .expect_err("opaque handles and callables must remain non-comparable");
        assert_eq!(diagnostic.code, expected_code, "{case}: {diagnostic:?}");
        assert_eq!(diagnostic.message, expected_message, "{case}");
        assert_eq!(diagnostic.help, expected_help, "{case}");
    }
}

#[test]
fn public_program_metadata_retrieves_the_checked_consuming_closure() {
    let temp = TempDir::new("aura-ffi-closure-metadata");
    let main_path = write_checked_ffi_package(
        &temp,
        r#"
extern "C" opaque class Handle
extern "C" def acquire() -> Handle

def main():
    handle = acquire()
    close_later: def() -> Handle = lambda: handle
"#,
        true,
    );

    let program = aura_compiler::check_path(&main_path)
        .expect("a captured handle should produce checked closure metadata");
    assert_eq!(program.closures.len(), 1);
    let (id, indexed) = program
        .closures
        .first_key_value()
        .expect("the lambda should have one closure entry");
    let retrieved = program
        .closure_info(id)
        .expect("the public metadata lookup should retrieve the checked closure");
    assert_eq!(retrieved.id, indexed.id);
    assert_eq!(
        retrieved.call_kind,
        aura_compiler::sema::ClosureCallKind::Consuming
    );
    assert_eq!(retrieved.return_type.to_string(), "Handle");
    assert_eq!(retrieved.captures.len(), 1);
    assert_eq!(retrieved.captures[0].name, "handle");
    assert_eq!(retrieved.captures[0].ty.to_string(), "Handle");
}

#[test]
fn public_from_import_preserves_extern_call_and_handle_identity_contracts() {
    let temp = TempDir::new("aura-ffi-from-import-contract");
    let main_path = write_checked_ffi_package(
        &temp,
        r#"
from native import Handle
from native import acquire

def main():
    handle: Handle = acquire()
"#,
        true,
    );
    fs::write(
        temp.path.join("src/native.au"),
        concat!(
            "public extern \"C\" opaque class Handle\n",
            "public extern \"C\" def acquire() -> Handle\n",
        ),
    )
    .expect("native module should be written");

    let program = aura_compiler::check_path(&main_path)
        .expect("from-imported extern calls should return the imported canonical handle type");
    assert_eq!(
        program
            .canonical_type_names
            .get("Handle")
            .map(String::as_str),
        Some("native.Handle")
    );
    assert_eq!(
        program.extern_functions["acquire"]
            .signature
            .return_type
            .to_string(),
        "native.Handle"
    );
}

#[test]
fn public_module_exports_preserve_function_and_tuple_callable_contracts() {
    let temp = TempDir::new("aura-exported-callable-contracts");
    let main_path = write_checked_ffi_package(
        &temp,
        r#"
import helpers

def increment(value: int32) -> int32:
    return value + 1

def main():
    applied: int32 = helpers.apply(increment, 4)
    total: int32 = helpers.sum_pair((2, 3))
"#,
        true,
    );
    fs::write(
        temp.path.join("src/helpers.au"),
        r#"
public def apply(callback: def(int32) -> int32, value: int32) -> int32:
    return callback(value)

public def sum_pair(pair: (int32, int32)) -> int32:
    return pair[0] + pair[1]
"#,
    )
    .expect("helper module should be written");

    let program = aura_compiler::check_path(&main_path)
        .expect("exported function and tuple contracts should remain callable after qualification");
    let helpers = &program.imported_modules["helpers"];
    assert_eq!(
        helpers.functions["apply"].signature.params[0].to_string(),
        "def(int32) -> int32"
    );
    assert_eq!(
        helpers.functions["sum_pair"].signature.params[0].to_string(),
        "(int32, int32)"
    );
}
