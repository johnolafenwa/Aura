#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP_PACKAGE_ID: AtomicU64 = AtomicU64::new(0);

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
}

struct TempPackage {
    path: PathBuf,
}

impl TempPackage {
    fn new(allow_ffi: bool) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should follow the Unix epoch")
            .as_nanos();
        Self::new_at_timestamp(allow_ffi, timestamp)
    }

    fn new_at_timestamp(allow_ffi: bool, timestamp: u128) -> Self {
        let path = loop {
            let sequence = NEXT_TEMP_PACKAGE_ID.fetch_add(1, Ordering::Relaxed);
            let unique = format!(
                "aura-ffi-acceptance-{}-{timestamp}-{sequence}",
                std::process::id()
            );
            let candidate = std::env::temp_dir().join(unique);
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("FFI acceptance package directory should exist: {error}"),
            }
        };
        fs::create_dir(path.join("src"))
            .expect("FFI acceptance package source directory should exist");
        let opt_in = if allow_ffi { "allow_ffi = true\n" } else { "" };
        fs::write(
            path.join("Aura.toml"),
            format!(
                "[package]\nname = \"ffi_acceptance\"\nversion = \"0.1.0\"\nedition = \"2026\"\n{opt_in}"
            ),
        )
        .expect("FFI acceptance manifest should be writable");
        Self { path }
    }

    /// Appends `settings` to the manifest.
    fn manifest(&self, settings: &str) {
        let path = self.path.join("Aura.toml");
        let mut manifest = fs::read_to_string(&path).expect("the manifest should be readable");
        manifest.push_str(settings);
        fs::write(&path, manifest).expect("the manifest should be writable");
    }

    /// Builds the C `source` as the shared library `name` in the package's
    /// `native` directory.
    fn c_library(&self, name: &str, source: &str) {
        let directory = self.path.join("native");
        fs::create_dir_all(&directory).expect("the native directory should exist");
        let c_file = directory.join(format!("{name}.c"));
        fs::write(&c_file, source).expect("the C source should be writable");
        let (kind, extension) = if cfg!(target_os = "macos") {
            ("-dynamiclib", "dylib")
        } else {
            ("-shared", "so")
        };
        let output = Command::new("cc")
            .args([kind, "-fPIC", "-O1", "-o"])
            .arg(directory.join(format!("lib{name}.{extension}")))
            .arg(&c_file)
            .output()
            .expect("the C compiler should start");
        assert_success(&output, "building the C fixture library");
    }

    fn source(&self, source: &str) -> PathBuf {
        let path = self.path.join("src/main.au");
        fs::write(&path, source).expect("FFI acceptance source should be writable");
        path
    }
}

#[test]
fn same_clock_tick_temp_packages_remain_isolated() {
    let first = TempPackage::new_at_timestamp(false, 1);
    let second = TempPackage::new_at_timestamp(false, 1);

    assert_ne!(first.path, second.path);
    let first_source = first.source("def main():\n    print(1)\n");
    let second_source = second.source("def test_only():\n    pass\n");
    assert_eq!(
        fs::read_to_string(first_source).expect("first source should remain readable"),
        "def main():\n    print(1)\n"
    );
    assert_eq!(
        fs::read_to_string(second_source).expect("second source should remain readable"),
        "def test_only():\n    pass\n"
    );
}

impl Drop for TempPackage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} should succeed; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_mir(source: &Path) -> Output {
    Command::new(aura_bin())
        .arg("run")
        .arg("--backend")
        .arg("mir")
        .arg(source)
        .output()
        .expect("forced MIR FFI acceptance run should start")
}

fn build_direct(source: &Path, binary: &Path) -> Output {
    Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(binary)
        .arg(source)
        .output()
        .expect("direct FFI acceptance build should start")
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root should be discoverable from the aura crate")
}

#[test]
fn getpid_binding_runs_with_backend_parity() {
    let package = TempPackage::new(true);
    let source = package.source(
        r#"public extern "C" def getpid() -> int32

def main() -> int32:
    print(getpid() > 0)
    return 0
"#,
    );

    let mir = run_mir(&source);
    assert_success(&mir, "forced MIR FFI acceptance run");
    assert_eq!(mir.stdout, b"true\n");

    let binary = package.path.join("ffi-acceptance-direct");
    let build = build_direct(&source, &binary);
    assert_success(&build, "direct FFI acceptance build");
    let direct = Command::new(binary)
        .output()
        .expect("direct FFI acceptance binary should start");
    assert_success(&direct, "direct FFI acceptance run");
    assert_eq!(direct.stdout, b"true\n");
    assert_eq!(direct.stdout, mir.stdout);
}

#[test]
fn maintained_getpid_example_runs_with_backend_parity() {
    let package = repository_root().join("examples/packages/ffi_getpid");
    let source = package.join("src/main.au");
    assert!(source.is_file(), "maintained FFI example source is missing");
    assert!(
        package.join("Aura.toml").is_file(),
        "maintained FFI example manifest is missing"
    );

    let mir = run_mir(&source);
    assert_success(&mir, "maintained FFI example MIR run");
    assert_eq!(mir.stdout, b"true\n");

    let output_dir = TempPackage::new(false);
    let binary = output_dir.path.join("ffi-getpid-example-direct");
    let build = build_direct(&source, &binary);
    assert_success(&build, "maintained FFI example direct build");
    let direct = Command::new(binary)
        .output()
        .expect("maintained FFI example direct binary should start");
    assert_success(&direct, "maintained FFI example direct run");
    assert_eq!(direct.stdout, b"true\n");
    assert_eq!(direct.stdout, mir.stdout);
}

#[test]
fn manifest_authorized_ffi_test_function_runs_through_the_trusted_path_api() {
    let package = TempPackage::new(true);
    let source = package.source(
        r#"public extern "C" def getpid() -> int32

def test_getpid():
    assert getpid() > 0
"#,
    );

    let output = Command::new(aura_bin())
        .arg("test")
        .arg(&source)
        .output()
        .expect("manifest-authorized FFI test should start");
    assert_success(&output, "manifest-authorized FFI test");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn package_ffi_requires_explicit_manifest_opt_in() {
    let package = TempPackage::new(false);
    let source = package.source(
        r#"public extern "C" def getpid() -> int32

def main() -> int32:
    return getpid()
"#,
    );

    let output = Command::new(aura_bin())
        .arg("check")
        .arg(&source)
        .output()
        .expect("FFI manifest rejection check should start");
    assert!(
        !output.status.success(),
        "FFI without opt-in must be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("AU2999"), "{stderr}");
    assert!(stderr.contains("allow_ffi = true"), "{stderr}");
}

#[test]
fn ffi_nullable_handle_results_construct_none_or_the_owned_handle_with_backend_parity() {
    let package = TempPackage::new(true);
    let source = package.source(
        r#"extern "C" opaque class Block
extern "C" def calloc(count: uint64, size: uint64) -> Block | None
extern "C" def free(block: own Block) -> None

def describe(block: own Block | None) -> str:
    if block is None:
        return "null"
    free(block)
    return "allocated"

def main() -> int32:
    print(describe(calloc(1, 8)))
    print(describe(calloc(18446744073709551615, 18446744073709551615)))
    return 0
"#,
    );

    let mir = run_mir(&source);
    assert_success(&mir, "forced MIR nullable-handle acceptance run");
    assert_eq!(mir.stdout, b"allocated\nnull\n");

    let binary = package.path.join("ffi-nullable-handle-direct");
    let build = build_direct(&source, &binary);
    assert_success(&build, "direct nullable-handle acceptance build");
    let direct = Command::new(binary)
        .output()
        .expect("direct nullable-handle acceptance binary should start");
    assert_success(&direct, "direct nullable-handle acceptance run");
    assert_eq!(direct.stdout, mir.stdout);
}

#[test]
fn ffi_nullable_handle_non_optional_null_results_still_trap_with_backend_parity() {
    let package = TempPackage::new(true);
    let source = package.source(
        r#"extern "C" opaque class Block
extern "C" def calloc(count: uint64, size: uint64) -> Block
extern "C" def free(block: own Block) -> None

def main() -> int32:
    free(calloc(18446744073709551615, 18446744073709551615))
    print("unreachable")
    return 0
"#,
    );

    let mir = run_mir(&source);
    assert!(
        !mir.status.success(),
        "a null `-> Handle` result must still trap on the MIR backend"
    );
    let stderr = String::from_utf8_lossy(&mir.stderr);
    assert!(stderr.contains("AU4005"), "{stderr}");
    assert!(stderr.contains("null opaque handle"), "{stderr}");
    assert!(!String::from_utf8_lossy(&mir.stdout).contains("unreachable"));

    let binary = package.path.join("ffi-null-handle-direct");
    let build = build_direct(&source, &binary);
    assert_success(&build, "direct null-handle acceptance build");
    let direct = Command::new(binary)
        .output()
        .expect("direct null-handle acceptance binary should start");
    assert!(
        !direct.status.success(),
        "a null `-> Handle` result must still trap on the direct backend"
    );
    let stderr = String::from_utf8_lossy(&direct.stderr);
    assert!(stderr.contains("AU4005"), "{stderr}");
    assert!(stderr.contains("null opaque handle"), "{stderr}");
    assert!(!String::from_utf8_lossy(&direct.stdout).contains("unreachable"));
}

#[test]
fn ffi_nullable_handle_results_still_require_the_manifest_opt_in() {
    let package = TempPackage::new(false);
    let source = package.source(
        r#"extern "C" opaque class Block
extern "C" def find_block() -> Block | None

def main() -> int32:
    return 0
"#,
    );

    let output = Command::new(aura_bin())
        .arg("check")
        .arg(&source)
        .output()
        .expect("nullable-handle manifest rejection check should start");
    assert!(
        !output.status.success(),
        "a nullable-handle extern without opt-in must be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("AU2999"), "{stderr}");
    assert!(stderr.contains("allow_ffi = true"), "{stderr}");
    assert!(!stderr.contains("AU2010"), "{stderr}");
}

const FIXTURE_LIBRARY: &str = r#"
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

int64_t fx_widen_i8(int8_t v) { return v; }
int64_t fx_widen_i16(int16_t v) { return v; }
int64_t fx_widen_i32(int32_t v) { return v; }
uint64_t fx_widen_u8(uint8_t v) { return v; }
uint64_t fx_widen_u16(uint16_t v) { return v; }
uint64_t fx_widen_u32(uint32_t v) { return v; }
int8_t fx_i8(int64_t v) { return (int8_t)v; }
int16_t fx_i16(int64_t v) { return (int16_t)v; }
int32_t fx_i32(int64_t v) { return (int32_t)v; }
uint8_t fx_u8(int64_t v) { return (uint8_t)v; }
uint16_t fx_u16(int64_t v) { return (uint16_t)v; }
uint32_t fx_u32(int64_t v) { return (uint32_t)v; }
uint64_t fx_u64(uint64_t v) { return v + 1; }
bool fx_not(bool v) { return !v; }
uint8_t fx_bad_bool(void) { return 2; }
float fx_half(float v) { return v / 2.0f; }
double fx_mix(bool a, int8_t b, int16_t c, int32_t d, int64_t e, uint8_t f,
              uint16_t g, uint32_t h, uint64_t i, float j, double k) {
    return (double)a + b + c + d + (double)e + f + g + h + (double)i + j + k;
}
int64_t fx_many(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e,
                int64_t f, int64_t g, int64_t h, int64_t i, int64_t j) {
    return a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g + 8 * h + 9 * i + 10 * j;
}
double fx_floats(double a, double b, double c, double d, double e, double f,
                 double g, double h, double i, double j) {
    return a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g + 8 * h + 9 * i + 10 * j;
}
uint64_t fx_sum(const uint8_t *bytes, uint64_t length) {
    if (bytes == NULL) return 1000 + length;
    uint64_t sum = 0;
    for (uint64_t i = 0; i < length; i++) sum += bytes[i];
    return sum;
}
uint64_t fx_pair(const uint8_t *a, uint64_t an, const uint8_t *b, uint64_t bn) {
    return fx_sum(a, an) * 100000 + fx_sum(b, bn);
}
void fx_fill(uint8_t *bytes, uint64_t length) {
    for (uint64_t i = 0; i < length; i++) bytes[i] = (uint8_t)(bytes[i] * 2 + i);
}
typedef struct { int64_t value; } fx_box;
static int64_t fx_live = 0;
fx_box *fx_open(int64_t value) {
    if (value < 0) return NULL;
    fx_box *box = malloc(sizeof(fx_box));
    box->value = value;
    fx_live++;
    return box;
}
int64_t fx_read(const fx_box *box) { return box->value; }
void fx_add(fx_box *box, int64_t more) { box->value += more; }
void fx_close(fx_box *box) { free(box); fx_live--; }
int64_t fx_live_boxes(void) { return fx_live; }
uint64_t fx_strlen(const char *text) { return text ? strlen(text) : 9999; }
const char *fx_name(int64_t which) {
    if (which == 0) return NULL;
    if (which == 1) return "h\xc3\xa9llo";
    if (which == 2) return "\xff\xfe";
    return "";
}
static char fx_buffer[64];
const char *fx_echo(const char *text) {
    strncpy(fx_buffer, text, sizeof(fx_buffer) - 1);
    return fx_buffer;
}
uint64_t fx_join(const char *a, const uint8_t *b, uint64_t bn, const char *c) {
    return strlen(a) * 10000 + fx_sum(b, bn) * 100 + (c ? strlen(c) : 99);
}
bool fx_divide(int32_t a, int32_t b, int32_t *quotient, int8_t *remainder) {
    if (b == 0) return false;
    *quotient = a / b;
    *remainder = (int8_t)(a % b);
    return true;
}
void fx_bump(bool *flag, uint8_t *a, int16_t *b, uint32_t *c, int64_t *d,
             uint64_t *e, float *f, double *g) {
    *flag = !*flag;
    *a += 1;
    *b -= 1;
    *c += 1;
    *d -= 1;
    *e += 1;
    *f /= 2.0f;
    *g *= 2.0;
}
void fx_narrow(int8_t *a, int16_t *b, int32_t *c) {
    *a = (int8_t)(*a - 1);
    *b = (int16_t)(*b - 1);
    *c = *c - 1;
}
void fx_bad_flag(bool *flag) { *(uint8_t *)flag = 7; }
int32_t fx_create(int64_t value, fx_box **out) {
    *out = fx_open(value);
    return *out ? 0 : 1;
}
int64_t fx_read_or(const fx_box *box, int64_t fallback) {
    return box ? box->value : fallback;
}
void fx_clear(fx_box **slot) {
    if (*slot) fx_close(*slot);
    *slot = NULL;
}
"#;

const FIXTURE_DECLARATIONS: &str = r#"extern "C" opaque class Box

extern "C" def fx_widen_i8(v: int8) -> int64
extern "C" def fx_widen_i16(v: int16) -> int64
extern "C" def fx_widen_i32(v: int32) -> int64
extern "C" def fx_widen_u8(v: uint8) -> uint64
extern "C" def fx_widen_u16(v: uint16) -> uint64
extern "C" def fx_widen_u32(v: uint32) -> uint64
extern "C" def fx_i8(v: int64) -> int8
extern "C" def fx_i16(v: int64) -> int16
extern "C" def fx_i32(v: int64) -> int32
extern "C" def fx_u8(v: int64) -> uint8
extern "C" def fx_u16(v: int64) -> uint16
extern "C" def fx_u32(v: int64) -> uint32
extern "C" def fx_u64(v: uint64) -> uint64
extern "C" def fx_not(v: bool) -> bool
extern "C" def fx_bad_bool() -> bool
extern "C" def fx_half(v: float32) -> float32
extern "C" def fx_mix(
    a: bool, b: int8, c: int16, d: int32, e: int64, f: uint8,
    g: uint16, h: uint32, i: uint64, j: float32, k: float64
) -> float64
extern "C" def fx_many(
    a: int64, b: int64, c: int64, d: int64, e: int64,
    f: int64, g: int64, h: int64, i: int64, j: int64
) -> int64
extern "C" def fx_floats(
    a: float64, b: float64, c: float64, d: float64, e: float64,
    f: float64, g: float64, h: float64, i: float64, j: float64
) -> float64
extern "C" def text_sum(text: str) -> uint64 symbol "fx_sum"
extern "C" def fx_sum(bytes: list[uint8]) -> uint64
extern "C" def fx_pair(a: str, b: list[uint8]) -> uint64
extern "C" def fx_fill(bytes: mut list[uint8]) -> None
extern "C" def fx_open(value: int64) -> Box
extern "C" def fx_find(value: int64) -> Box | None symbol "fx_open"
extern "C" def fx_read(box: Box) -> int64
extern "C" def fx_add(box: Box, more: int64) -> None
extern "C" def fx_close(box: own Box) -> None
extern "C" def fx_live_boxes() -> int64
extern "C" def fx_strlen(text: cstr) -> uint64
extern "C" def fx_strlen_or(text: cstr | None) -> uint64 symbol "fx_strlen"
extern "C" def fx_name(which: int64) -> cstr
extern "C" def fx_maybe_name(which: int64) -> cstr | None symbol "fx_name"
extern "C" def fx_echo(text: cstr) -> cstr
extern "C" def fx_join(a: cstr, b: list[uint8], c: cstr | None) -> uint64
extern "C" def fx_divide(a: int32, b: int32, quotient: mut int32, remainder: mut int8) -> bool
extern "C" def fx_bump(
    flag: mut bool, a: mut uint8, b: mut int16, c: mut uint32, d: mut int64,
    e: mut uint64, f: mut float32, g: mut float64
) -> None
extern "C" def fx_narrow(a: mut int8, b: mut int16, c: mut int32) -> None
extern "C" def fx_bad_flag(flag: mut bool) -> None
extern "C" def fx_create(value: int64, out: mut Box | None) -> int32
extern "C" def fx_read_or(box: Box | None, fallback: int64) -> int64
extern "C" def fx_clear(slot: mut Box | None) -> None
"#;

/// A package that links the fixture library from its `native` directory.
fn fixture_package(main: &str) -> (TempPackage, PathBuf) {
    let package = TempPackage::new(true);
    package.c_library("aurafixture", FIXTURE_LIBRARY);
    package.manifest("\n[ffi]\nlibraries = [\"aurafixture\"]\nlibrary_paths = [\"native\"]\n");
    let source = package.source(&format!("{FIXTURE_DECLARATIONS}\n{main}"));
    (package, source)
}

/// Runs the package on both backends and returns each one's output. The
/// built program runs from another directory, so it must find the library
/// through the path the build recorded.
fn run_on_both_backends(package: &TempPackage, source: &Path) -> (Output, Output) {
    let mir = run_mir(source);
    let binary = package.path.join("ffi-fixture-direct");
    let build = build_direct(source, &binary);
    assert_success(&build, "direct FFI fixture build");
    let direct = Command::new(&binary)
        .current_dir(std::env::temp_dir())
        .output()
        .expect("direct FFI fixture binary should start");
    (mir, direct)
}

#[test]
fn a_linked_library_passes_every_boundary_type_with_backend_parity() {
    let (package, source) = fixture_package(
        r#"
def describe(found: own Box | None) -> str:
    if found is None:
        return "none"
    value = fx_read(found)
    fx_close(found)
    return f"box {value}"

def main() -> int32:
    print(fx_widen_i8(-5))
    print(fx_widen_i16(-300))
    print(fx_widen_i32(-70000))
    print(fx_widen_u8(250))
    print(fx_widen_u16(65000))
    print(fx_widen_u32(4000000000))
    print(fx_i8(-5))
    print(fx_i8(200))
    print(fx_i16(-300))
    print(fx_i16(40000))
    print(fx_i32(-70000))
    print(fx_i32(3000000000))
    print(fx_u8(250))
    print(fx_u8(-1))
    print(fx_u16(65000))
    print(fx_u16(-1))
    print(fx_u32(4000000000))
    print(fx_u32(-1))
    print(fx_u64(18446744073709551614))
    print(fx_not(true))
    print(fx_not(false))
    print(fx_half(3.0))
    print(fx_mix(true, -2, -300, -70000, 5000000000, 250, 65000, 4000000000, 7, 0.5, 0.25))
    print(fx_many(1, 2, 3, 4, 5, 6, 7, 8, 9, 10))
    print(fx_floats(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.5))
    held = "abc"
    print(text_sum(held))
    print(text_sum("abd"))
    print(text_sum(""))
    empty = ""
    print(text_sum(empty))
    mut bytes: list[uint8] = [1, 2, 3]
    print(fx_sum(bytes))
    none: list[uint8] = []
    print(fx_sum(none))
    print(fx_pair(held, bytes))
    fx_fill(bytes)
    print(bytes)
    print(len(bytes))
    box = fx_open(40)
    fx_add(box, 2)
    print(fx_read(box))
    print(fx_live_boxes())
    fx_close(box)
    print(describe(fx_find(7)))
    print(describe(fx_find(-1)))
    print(fx_live_boxes())
    return 0
"#,
    );
    let (mir, direct) = run_on_both_backends(&package, &source);
    assert_success(&mir, "forced MIR FFI fixture run");
    assert_success(&direct, "direct FFI fixture run");
    let expected = "\
-5\n-300\n-70000\n250\n65000\n4000000000\n\
-5\n-56\n-300\n-25536\n-70000\n-1294967296\n\
250\n255\n65000\n65535\n4000000000\n4294967295\n\
18446744073709551615\nfalse\ntrue\n1.5\n\
8999994956.75\n385\n390.0\n\
294\n295\n1000\n1000\n6\n1000\n29400006\n\
[2, 5, 8]\n3\n42\n1\nbox 7\nnone\n0\n";
    assert_eq!(String::from_utf8_lossy(&mir.stdout), expected);
    assert_eq!(String::from_utf8_lossy(&direct.stdout), expected);
}

#[test]
fn c_text_out_parameters_and_nullable_handles_cross_with_backend_parity() {
    let (package, source) = fixture_package(
        r#"
def show(text: str | None) -> str:
    if text is None:
        return "none"
    return f"<{text}>"

def main() -> int32:
    print(fx_strlen("héllo"))
    held = "abc"
    print(fx_strlen(held))
    print(fx_strlen(""))
    print(fx_strlen_or(None))
    print(fx_strlen_or(held))
    print(fx_strlen_or("literal"))
    maybe: str | None = "four"
    print(fx_strlen_or(maybe))
    print(fx_name(1))
    print(len(fx_name(3)))
    print(show(fx_maybe_name(1)))
    print(show(fx_maybe_name(0)))
    print(fx_echo(held + "def"))
    bytes: list[uint8] = [1, 2, 3]
    print(fx_join(held, bytes, None))
    print(fx_join("ab", bytes, held))

    mut quotient: int32 = 0
    mut remainder: int8 = 0
    print(fx_divide(-17, 5, quotient, remainder))
    print(quotient)
    print(remainder)
    print(fx_divide(1, 0, quotient, remainder))
    print(quotient)
    mut flag = false
    mut a: uint8 = 255
    mut b: int16 = -32768
    mut c: uint32 = 4294967294
    mut d: int64 = -9000000000
    mut e: uint64 = 18446744073709551614
    mut f: float32 = 3.0
    mut g = 1.25
    fx_bump(flag, a, b, c, d, e, f, g)
    print(flag)
    print(a)
    print(b)
    print(c)
    print(d)
    print(e)
    print(f)
    print(g)
    mut n8: int8 = 0
    mut n16: int16 = 0
    mut n32: int32 = 0
    fx_narrow(n8, n16, n32)
    print(n8)
    print(n16)
    print(n32)

    mut slot: Box | None = None
    print(fx_create(41, slot))
    print(fx_read_or(slot, -1))
    print(fx_read_or(None, -1))
    print(fx_live_boxes())
    fx_clear(slot)
    print(slot is None)
    print(fx_live_boxes())
    print(fx_create(-1, slot))
    print(slot is None)
    mut round = 0
    while round < 1000:
        fx_create(round, slot)
        fx_clear(slot)
        round += 1
    print(fx_live_boxes())
    return 0
"#,
    );
    let (mir, direct) = run_on_both_backends(&package, &source);
    assert_success(&mir, "forced MIR C text fixture run");
    assert_success(&direct, "direct C text fixture run");
    let expected = "\
6\n3\n0\n9999\n3\n7\n4\nhéllo\n0\n<héllo>\nnone\nabcdef\n\
30699\n20603\n\
true\n-3\n-2\nfalse\n-3\n\
true\n0\n32767\n4294967295\n-9000000001\n18446744073709551615\n1.5\n2.5\n\
-1\n-1\n-1\n\
0\n41\n-1\n1\ntrue\n0\n1\ntrue\n0\n";
    assert_eq!(String::from_utf8_lossy(&mir.stdout), expected);
    assert_eq!(String::from_utf8_lossy(&direct.stdout), expected);
}

#[test]
fn foreign_results_a_declaration_rules_out_trap_with_backend_parity() {
    for (main, code, detail) in [
        (
            "def main() -> int32:\n    print(fx_bad_bool())\n    return 0\n",
            "AU4001",
            "FFI call to `fx_bad_bool` failed",
        ),
        (
            "def main() -> int32:\n    box = fx_open(-1)\n    fx_close(box)\n    return 0\n",
            "AU4005",
            "FFI call to `fx_open` failed",
        ),
        (
            "def main() -> int32:\n    print(fx_name(0))\n    return 0\n",
            "AU4005",
            "FFI call to `fx_name` failed: FFI function returned a null cstr",
        ),
        (
            "def main() -> int32:\n    print(fx_name(2))\n    return 0\n",
            "AU4005",
            "FFI call to `fx_name` failed: FFI function returned a cstr that is not UTF-8",
        ),
        (
            "def main() -> int32:\n    print(fx_maybe_name(2))\n    return 0\n",
            "AU4005",
            "FFI call to `fx_name` failed: FFI function returned a cstr that is not UTF-8",
        ),
        (
            "def main() -> int32:\n    print(fx_strlen(\"a\\0b\"))\n    return 0\n",
            "AU4005",
            "FFI call to `fx_strlen` failed: FFI argument 1 is a cstr that contains a NUL byte",
        ),
        (
            "def main() -> int32:\n    held = \"a\\0b\"\n    print(fx_join(\"x\", [], held))\n    return 0\n",
            "AU4005",
            "FFI call to `fx_join` failed: FFI argument 3 is a cstr that contains a NUL byte",
        ),
        (
            "def main() -> int32:\n    mut flag = true\n    fx_bad_flag(flag)\n    print(flag)\n    return 0\n",
            "AU4001",
            "FFI call to `fx_bad_flag` failed",
        ),
    ] {
        let (package, source) = fixture_package(main);
        let (mir, direct) = run_on_both_backends(&package, &source);
        for (backend, output) in [("mir", &mir), ("direct", &direct)] {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(!output.status.success(), "{backend}: {stderr}");
            assert!(
                stderr.contains(code) && stderr.contains(detail),
                "{backend} should report {code}: {stderr}"
            );
        }
    }
}

#[test]
fn a_library_the_host_lacks_fails_the_build_and_the_interpreted_run() {
    let package = TempPackage::new(true);
    package.manifest("\n[ffi]\nlibraries = [\"aura_no_such_library\"]\n");
    let source = package.source(
        "extern \"C\" def getpid() -> int32\n\ndef main() -> int32:\n    print(getpid() > 0)\n    return 0\n",
    );

    let mir = run_mir(&source);
    let stderr = String::from_utf8_lossy(&mir.stderr);
    assert!(!mir.status.success(), "{stderr}");
    assert!(
        stderr.contains("AU4005")
            && stderr.contains(
                "cannot start the program: FFI library `aura_no_such_library` could not be loaded"
            ),
        "{stderr}"
    );

    let build = build_direct(&source, &package.path.join("ffi-missing-library"));
    let stderr = String::from_utf8_lossy(&build.stderr);
    assert!(!build.status.success(), "{stderr}");
    assert!(stderr.contains("aura_no_such_library"), "{stderr}");
}

#[test]
fn link_settings_require_the_manifest_opt_in_and_bare_names() {
    let package = TempPackage::new(false);
    package.manifest("\n[ffi]\nlibraries = [\"m\"]\n");
    let source = package.source("def main() -> int32:\n    return 0\n");
    let output = run_mir(&source);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("AU2999")
            && stderr.contains("names native libraries in `[ffi]` but does not opt in to FFI"),
        "{stderr}"
    );

    let package = TempPackage::new(true);
    package.manifest("\n[ffi]\nlibraries = [\"-Wl,--version\"]\n");
    let source = package.source("def main() -> int32:\n    return 0\n");
    for output in [
        run_mir(&source),
        build_direct(&source, &package.path.join("ffi-bad-name")),
    ] {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(
            stderr.contains("AU2999") && stderr.contains("has invalid entry `-Wl,--version`"),
            "{stderr}"
        );
    }
}

#[test]
fn host_tables_link_only_on_the_host_they_name() {
    let package = TempPackage::new(true);
    package.c_library("aurafixture", FIXTURE_LIBRARY);
    let (host, other) = if cfg!(target_os = "macos") {
        ("macos", "linux")
    } else {
        ("linux", "macos")
    };
    package.manifest(&format!(
        "\n[ffi.{host}]\nlibraries = [\"aurafixture\"]\nlibrary_paths = [\"native\"]\n\n[ffi.{other}]\nlibraries = [\"aura_no_such_library\"]\n"
    ));
    let source = package.source(&format!(
        "{FIXTURE_DECLARATIONS}\ndef main() -> int32:\n    print(fx_many(1, 1, 1, 1, 1, 1, 1, 1, 1, 1))\n    return 0\n"
    ));
    let (mir, direct) = run_on_both_backends(&package, &source);
    assert_success(&mir, "forced MIR host-table run");
    assert_success(&direct, "direct host-table run");
    assert_eq!(mir.stdout, b"55\n");
    assert_eq!(direct.stdout, b"55\n");
}
