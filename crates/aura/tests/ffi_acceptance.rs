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

/// Whether the host C toolchain links the system SQLite library.
fn host_links_sqlite() -> bool {
    let scratch = TempPackage::new(false);
    let c_file = scratch.path.join("probe.c");
    fs::write(
        &c_file,
        "const char *sqlite3_libversion(void);\nint main(void) { return sqlite3_libversion() == 0; }\n",
    )
    .expect("the probe source should be writable");
    Command::new("cc")
        .arg(&c_file)
        .arg("-lsqlite3")
        .arg("-o")
        .arg(scratch.path.join("probe"))
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn maintained_sqlite_example_runs_with_backend_parity() {
    if !host_links_sqlite() {
        eprintln!("the host has no SQLite library to link; the example needs one");
        return;
    }
    let source = repository_root().join("examples/packages/sqlite_notes/src/main.au");
    let expected = "1\n2\n3\n1: water the plants\n3: renew the library books\n\
                    a row of 2 columns\nno such table: missing\n0\n";

    let mir = run_mir(&source);
    assert_success(&mir, "SQLite example MIR run");
    assert_eq!(String::from_utf8_lossy(&mir.stdout), expected);

    let output_dir = TempPackage::new(false);
    let binary = output_dir.path.join("sqlite-notes-direct");
    let build = build_direct(&source, &binary);
    assert_success(&build, "SQLite example direct build");
    let direct = Command::new(binary)
        .output()
        .expect("SQLite example direct binary should start");
    assert_success(&direct, "SQLite example direct run");
    assert_eq!(String::from_utf8_lossy(&direct.stdout), expected);
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
double fx_dot(const double *a, const double *b, int64_t n) {
    double total = 0.0;
    for (int64_t i = 0; i < n; i++) total += a[i] * b[i];
    return total;
}
void fx_scale(double *v, int64_t n, double k) {
    for (int64_t i = 0; i < n; i++) v[i] *= k;
}
int64_t fx_sum_i32(const int32_t *v, int64_t n) {
    int64_t total = 0;
    for (int64_t i = 0; i < n; i++) total += v[i];
    return total;
}
void fx_count_down(int16_t *v, int64_t n) {
    for (int64_t i = 0; i < n; i++) v[i] = (int16_t)(v[i] - i - 1);
}
float fx_sum_f32(const float *v, int64_t n) {
    float total = 0.0f;
    for (int64_t i = 0; i < n; i++) total += v[i];
    return total;
}
void fx_halve(float *v, int64_t n) {
    for (int64_t i = 0; i < n; i++) v[i] /= 2.0f;
}
void fx_flip(bool *v, int64_t n) {
    for (int64_t i = 0; i < n; i++) v[i] = !v[i];
}
int64_t fx_count_true(const bool *v, int64_t n) {
    int64_t total = 0;
    for (int64_t i = 0; i < n; i++) total += v[i];
    return total;
}
uint64_t fx_max_u64(const uint64_t *v, int64_t n) {
    uint64_t best = 0;
    for (int64_t i = 0; i < n; i++) if (v[i] > best) best = v[i];
    return best;
}
void fx_widen_all(const int8_t *a, const uint8_t *b, const uint16_t *c,
                  const uint32_t *d, int64_t *out, int64_t n) {
    for (int64_t i = 0; i < n; i++) out[i] = a[i] + b[i] + c[i] + (int64_t)d[i];
}
double ax_dot(const double *a, const double *b, int64_t n) {
    double total = 0.0;
    for (int64_t i = 0; i < n; i++) total += a[i] * b[i];
    return total;
}
void ax_scale(double *v, int64_t n, double k) {
    for (int64_t i = 0; i < n; i++) v[i] *= k;
}
int64_t ax_sum_i32(const int32_t *v, int64_t n) {
    int64_t total = 0;
    for (int64_t i = 0; i < n; i++) total = total * 10 + v[i];
    return total;
}
void ax_count_i64(int64_t *v, int64_t n, int64_t start) {
    for (int64_t i = 0; i < n; i++) v[i] = start + i;
}
void ax_halve_f32(float *v, int64_t n) {
    for (int64_t i = 0; i < n; i++) v[i] *= 0.5f;
}
bool ax_is_null(const double *v) { return v == NULL; }
bool fx_is_null(const double *v) { return v == NULL; }
bool fx_is_null_mut(int32_t *v) { return v == NULL; }
void fx_bad_flags(bool *v) { *(uint8_t *)v = 7; }

typedef struct { double x, y; } Vec2;
typedef struct { int32_t a, b; } IPair;
typedef struct { int64_t n; double w; } Mixed;
typedef struct { float x, y, z; } Vec3;
typedef struct { int8_t tag; int64_t v; } Tagged;
typedef struct { int64_t a, b, c; } Triple;
typedef struct { Vec2 lo; Vec2 hi; } Rect;
typedef struct { bool on; uint8_t b; uint16_t s; uint32_t w; } Small;
typedef struct { IPair p; float f; } Nest;
typedef struct { float f; int32_t i; } FloatInt;
typedef struct { int64_t lo, hi; } Wide;
typedef struct { uint8_t r, g, b; } Rgb;
typedef struct { bool ok; } Flag;

double fx_vec2_len2(Vec2 v) { return v.x * v.x + v.y * v.y; }
Vec2 fx_vec2_add(Vec2 a, Vec2 b) { Vec2 r = {a.x + b.x, a.y + b.y}; return r; }
void fx_vec2_scale(Vec2 *v, double by) { v->x *= by; v->y *= by; }
double fx_vec2_dot(const Vec2 *a, const Vec2 *b) { return a->x * b->x + a->y * b->y; }
int64_t fx_ipair_diff(IPair p) { return (int64_t)p.a - p.b; }
IPair fx_ipair_swap(IPair p) { IPair r = {p.b, p.a}; return r; }
Mixed fx_mixed_bump(Mixed m) { Mixed r = {m.n + 1, m.w * 2.0}; return r; }
double fx_mixed_sum(Mixed m) { return (double)m.n + m.w; }
Vec3 fx_vec3_cross(Vec3 a, Vec3 b) {
    Vec3 r = {a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x};
    return r;
}
float fx_vec3_sum(Vec3 v) { return v.x + v.y + v.z; }
Tagged fx_tagged_next(Tagged t) { Tagged r = {(int8_t)(t.tag - 1), t.v * 3}; return r; }
int64_t fx_triple_sum(Triple t) { return t.a + t.b + t.c; }
Triple fx_triple_make(int64_t start) { Triple r = {start, start + 1, start + 2}; return r; }
Triple fx_triple_rotate(Triple t) { Triple r = {t.b, t.c, t.a}; return r; }
double fx_rect_area(Rect r) { return (r.hi.x - r.lo.x) * (r.hi.y - r.lo.y); }
Rect fx_rect_grow(Rect r, double by) {
    Rect g = {{r.lo.x - by, r.lo.y - by}, {r.hi.x + by, r.hi.y + by}};
    return g;
}
void fx_rect_move(Rect *r, Vec2 by) {
    r->lo.x += by.x; r->lo.y += by.y; r->hi.x += by.x; r->hi.y += by.y;
}
uint64_t fx_small_pack(Small s) {
    return ((uint64_t)s.on << 56) | ((uint64_t)s.b << 48) | ((uint64_t)s.s << 32) | s.w;
}
Small fx_small_make(uint32_t w) {
    Small s = {w % 2 == 1, (uint8_t)(w + 200), (uint16_t)(w + 65000), w * 1000000000u};
    return s;
}
Nest fx_nest_make(int32_t a, float f) { Nest n = {{a, -a}, f}; return n; }
double fx_nest_sum(Nest n) { return (double)n.p.a + n.p.b + n.f; }
FloatInt fx_float_int(FloatInt v) { FloatInt r = {v.f * 2.0f, v.i + 1}; return r; }
Rgb fx_rgb_invert(Rgb c) { Rgb r = {(uint8_t)(255 - c.r), (uint8_t)(255 - c.g), (uint8_t)(255 - c.b)}; return r; }
int64_t fx_spill_ints(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e, Wide w, int64_t f) {
    return a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * w.lo + 7 * w.hi + 8 * f;
}
double fx_spill_floats(double a, double b, double c, double d, double e, double f, double g,
                       Vec2 v, double h) {
    return a + b + c + d + e + f + g + 10.0 * v.x + 100.0 * v.y + 1000.0 * h;
}
double fx_mix_all(Vec2 a, IPair p, Mixed m, Triple t, Vec2 b, Wide w) {
    return a.x + a.y + p.a + p.b + (double)m.n + m.w + (double)(t.a + t.b + t.c)
        + b.x + b.y + (double)(w.lo + w.hi);
}
Triple fx_triple_from(Wide w, Vec2 v) { Triple r = {w.lo, w.hi, (int64_t)(v.x + v.y)}; return r; }
int64_t fx_fold(int64_t (*step)(int64_t, int32_t), int32_t count) {
    int64_t total = 0;
    for (int32_t i = 1; i <= count; i++) total = step(total, i);
    return total;
}
double fx_integrate(double (*f)(double), double lo, double hi, int32_t steps) {
    double width = (hi - lo) / steps, sum = 0.0;
    for (int32_t i = 0; i < steps; i++) sum += f(lo + (i + 0.5) * width) * width;
    return sum;
}
typedef void (*fx_visitor)(uint8_t, int16_t, uint16_t, uint32_t, uint64_t, int8_t, float, bool);
void fx_visit(fx_visitor visit) {
    visit(255, -32768, 65535, 4294967295u, 18446744073709551615ull, -128, 0.25f, true);
    visit(0, 32767, 0, 7, 9, 127, -1.5f, false);
}
int64_t fx_count_if(bool (*test)(int32_t), const int32_t *values, int64_t count) {
    int64_t found = 0;
    for (int64_t i = 0; i < count; i++) found += test(values[i]) ? 1 : 0;
    return found;
}
double fx_step_all(void (*next)(int32_t *, double *, bool *), int32_t count) {
    int32_t slot = 1; double sum = 0.5; bool odd = false; int32_t flips = 0;
    for (int32_t i = 0; i < count; i++) { next(&slot, &sum, &odd); flips += odd; }
    return slot + sum + 1000.0 * flips;
}
int64_t fx_narrow_results(int8_t (*a)(int8_t), uint16_t (*b)(uint16_t), int16_t (*c)(int16_t),
                          uint8_t (*d)(uint8_t), int32_t (*e)(int32_t), uint32_t (*f)(uint32_t)) {
    return (int64_t)a(-100) + (int64_t)b(65000) + (int64_t)c(-30000) + (int64_t)d(200)
        + (int64_t)e(-2000000000) + (int64_t)f(4000000000u);
}
float fx_blend(float (*mix)(float, float)) { return mix(1.5f, 2.25f) + mix(-4.0f, 0.5f); }
uint64_t fx_big(uint64_t (*f)(uint64_t)) { return f(18446744073709551000ull); }
int32_t fx_null_slot(int32_t (*f)(int32_t *)) { int32_t v = 5; return f(NULL) * 100 + f(&v) + v; }
bool fx_truthy(bool (*f)(bool)) {
    bool (*raw)(uint8_t) = (bool (*)(uint8_t))f;
    return raw(7);
}
static void (*fx_kept)(int32_t) = NULL;
void fx_keep(void (*f)(int32_t)) { fx_kept = f; }
void fx_fire(int32_t value) { if (fx_kept) fx_kept(value); }
int32_t fx_twice(int32_t (*f)(int32_t), int32_t value) { return f(f(value)); }
int32_t fx_until_failure(int32_t (*f)(int32_t)) {
    int32_t last = 0;
    for (int32_t i = 3; i >= -1; i--) last = f(i);
    return last;
}
typedef struct { int32_t id; } fx_crate;
static int32_t fx_crates = 0;
static int32_t fx_closed[64];
static int32_t fx_closes = 0;
fx_crate *fx_crate_open(int32_t id) {
    if (id < 0) return NULL;
    fx_crate *made = malloc(sizeof(fx_crate));
    made->id = id;
    fx_crates++;
    return made;
}
int32_t fx_crate_id(const fx_crate *held) { return held->id; }
int32_t fx_crate_close(fx_crate *held) {
    fx_closed[fx_closes++ % 64] = held->id;
    fx_crates--;
    free(held);
    return 0;
}
int32_t fx_crates_open(void) { return fx_crates; }
int32_t fx_crate_closes(void) { return fx_closes; }
int32_t fx_crate_closed(int32_t index) { return fx_closed[index]; }
int32_t fx_crate_make(int32_t id, fx_crate **out) { *out = fx_crate_open(id); return *out ? 0 : 1; }
void fx_crate_keep(fx_crate **slot) { if (*slot) (*slot)->id += 100; }
void fx_crate_swap(fx_crate **slot, int32_t id) { fx_crate_close(*slot); *slot = fx_crate_open(id); }
Flag fx_bad_struct_flag(void) { Flag f; *(uint8_t *)&f.ok = 9; return f; }
void fx_bad_struct_out(Flag *f) { *(uint8_t *)&f->ok = 9; }
"#;

const FIXTURE_DECLARATIONS: &str = r#"extern "C" opaque class Box
extern "C" opaque class Crate drop fx_crate_close

extern "C" def fx_crate_open(id: int32) -> Crate
extern "C" def fx_crate_find(id: int32) -> Crate | None symbol "fx_crate_open"
extern "C" def fx_crate_id(held: Crate) -> int32
extern "C" def fx_crate_close(held: own Crate) -> int32
extern "C" def fx_crates_open() -> int32
extern "C" def fx_crate_closes() -> int32
extern "C" def fx_crate_closed(index: int32) -> int32
extern "C" def fx_crate_make(id: int32, out: mut Crate | None) -> int32
extern "C" def fx_crate_keep(slot: mut Crate | None) -> None
extern "C" def fx_crate_swap(slot: mut Crate | None, id: int32) -> None

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
extern "C" def fx_dot(a: ptr[float64], b: ptr[float64], n: int64) -> float64
extern "C" def fx_scale(v: mut ptr[float64], n: int64, k: float64) -> None
extern "C" def fx_sum_i32(v: ptr[int32], n: int64) -> int64
extern "C" def fx_count_down(v: mut ptr[int16], n: int64) -> None
extern "C" def fx_sum_f32(v: ptr[float32], n: int64) -> float32
extern "C" def fx_halve(v: mut ptr[float32], n: int64) -> None
extern "C" def fx_flip(v: mut ptr[bool], n: int64) -> None
extern "C" def fx_count_true(v: ptr[bool], n: int64) -> int64
extern "C" def fx_max_u64(v: ptr[uint64], n: int64) -> uint64
extern "C" def fx_widen_all(
    a: ptr[int8], b: ptr[uint8], c: ptr[uint16], d: ptr[uint32],
    out: mut ptr[int64], n: int64
) -> None
extern "C" def ax_dot(a: Array[float64], b: Array[float64], n: int64) -> float64
extern "C" def ax_scale(v: mut Array[float64], n: int64, k: float64) -> None
extern "C" def ax_sum_i32(v: Array[int32], n: int64) -> int64
extern "C" def ax_count_i64(v: mut Array[int64], n: int64, start: int64) -> None
extern "C" def ax_halve_f32(v: mut Array[float32], n: int64) -> None
extern "C" def ax_is_null(v: Array[float64]) -> bool
extern "C" def fx_is_null(v: ptr[float64]) -> bool
extern "C" def fx_is_null_mut(v: mut ptr[int32]) -> bool
extern "C" def fx_bad_flags(v: mut ptr[bool]) -> None

extern "C" class Vec2:
    x: float64
    y: float64

extern "C" class IPair:
    a: int32
    b: int32

extern "C" class Mixed:
    n: int64
    w: float64

extern "C" class Vec3:
    x: float32
    y: float32
    z: float32

extern "C" class Tagged:
    tag: int8
    v: int64

extern "C" class Triple:
    a: int64
    b: int64
    c: int64

extern "C" class Rect:
    lo: Vec2
    hi: Vec2

extern "C" class Small:
    on: bool
    b: uint8
    s: uint16
    w: uint32

extern "C" class Nest:
    p: IPair
    f: float32

extern "C" class FloatInt:
    f: float32
    i: int32

extern "C" class Wide:
    lo: int64
    hi: int64

extern "C" class Rgb:
    r: uint8
    g: uint8
    b: uint8

extern "C" class Flag:
    ok: bool

extern "C" def fx_vec2_len2(v: Vec2) -> float64
extern "C" def fx_vec2_add(a: Vec2, b: Vec2) -> Vec2
extern "C" def fx_vec2_scale(v: mut Vec2, by: float64) -> None
extern "C" def fx_vec2_dot(a: mut Vec2, b: mut Vec2) -> float64
extern "C" def fx_ipair_diff(p: IPair) -> int64
extern "C" def fx_ipair_swap(p: IPair) -> IPair
extern "C" def fx_mixed_bump(m: Mixed) -> Mixed
extern "C" def fx_mixed_sum(m: Mixed) -> float64
extern "C" def fx_vec3_cross(a: Vec3, b: Vec3) -> Vec3
extern "C" def fx_vec3_sum(v: Vec3) -> float32
extern "C" def fx_tagged_next(t: Tagged) -> Tagged
extern "C" def fx_triple_sum(t: Triple) -> int64
extern "C" def fx_triple_make(start: int64) -> Triple
extern "C" def fx_triple_rotate(t: Triple) -> Triple
extern "C" def fx_rect_area(r: Rect) -> float64
extern "C" def fx_rect_grow(r: Rect, by: float64) -> Rect
extern "C" def fx_rect_move(r: mut Rect, by: Vec2) -> None
extern "C" def fx_small_pack(s: Small) -> uint64
extern "C" def fx_small_make(w: uint32) -> Small
extern "C" def fx_nest_make(a: int32, f: float32) -> Nest
extern "C" def fx_nest_sum(n: Nest) -> float64
extern "C" def fx_float_int(v: FloatInt) -> FloatInt
extern "C" def fx_rgb_invert(c: Rgb) -> Rgb
extern "C" def fx_spill_ints(
    a: int64, b: int64, c: int64, d: int64, e: int64, w: Wide, f: int64
) -> int64
extern "C" def fx_spill_floats(
    a: float64, b: float64, c: float64, d: float64, e: float64, f: float64, g: float64,
    v: Vec2, h: float64
) -> float64
extern "C" def fx_mix_all(a: Vec2, p: IPair, m: Mixed, t: Triple, b: Vec2, w: Wide) -> float64
extern "C" def fx_triple_from(w: Wide, v: Vec2) -> Triple
extern "C" def fx_bad_struct_flag() -> Flag
extern "C" def fx_fold(step: def(total: int64, item: int32) -> int64, count: int32) -> int64
extern "C" def fx_integrate(
    f: def(x: float64) -> float64, lo: float64, hi: float64, steps: int32
) -> float64
extern "C" def fx_visit(
    visit: def(
        a: uint8, b: int16, c: uint16, d: uint32, e: uint64, f: int8, g: float32, h: bool
    ) -> None
) -> None
extern "C" def fx_count_if(test: def(value: int32) -> bool, values: ptr[int32], count: int64) -> int64
extern "C" def fx_step_all(
    next: def(slot: mut int32, sum: mut float64, odd: mut bool) -> None, count: int32
) -> float64
extern "C" def fx_narrow_results(
    a: def(v: int8) -> int8, b: def(v: uint16) -> uint16, c: def(v: int16) -> int16,
    d: def(v: uint8) -> uint8, e: def(v: int32) -> int32, f: def(v: uint32) -> uint32
) -> int64
extern "C" def fx_blend(mix: def(a: float32, b: float32) -> float32) -> float32
extern "C" def fx_big(f: def(v: uint64) -> uint64) -> uint64
extern "C" def fx_null_slot(f: def(slot: mut int32) -> int32) -> int32
extern "C" def fx_truthy(f: def(v: bool) -> bool) -> bool
extern "C" def fx_keep(f: def(value: int32) -> None) -> None
extern "C" def fx_fire(value: int32) -> None
extern "C" def fx_twice(f: def(value: int32) -> int32, value: int32) -> int32
extern "C" def fx_until_failure(f: def(value: int32) -> int32) -> int32
extern "C" def qsort(
    base: mut ptr[int32], count: uint64, size: uint64,
    compare: def(a: mut int32, b: mut int32) -> int32
) -> None
extern "C" def fx_bad_struct_out(f: mut Flag) -> None
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
fn pointer_parameters_share_list_elements_with_backend_parity() {
    let (package, source) = fixture_package(
        r#"
class Samples:
    values: list[float64]

def squares(count: int32) -> list[int32]:
    mut out: list[int32] = []
    mut i: int32 = 0
    while i < count:
        out.append(i * i)
        i += 1
    return out

def main() -> int32:
    xs: list[float64] = [1.0, 2.0, 3.0]
    ys: list[float64] = [4.0, 5.0, 6.0]
    print(fx_dot(xs, ys, 3))
    print(fx_dot(xs, xs, 2))
    print(fx_dot([0.5, 0.5], [2.0, 4.0], 2))

    mut zs: list[float64] = [1.0, 2.0, 3.0]
    kept = zs.copy()
    fx_scale(zs, 3, 2.5)
    print(zs)
    print(kept)
    fx_scale(zs, 1, 2.0)
    print(zs)

    grown = squares(100)
    print(fx_sum_i32(grown, len(grown)))
    print(fx_sum_i32(grown, 4))
    mut steps: list[int16] = [10, 20, 30, 40]
    fx_count_down(steps, 4)
    print(steps)
    mut halves: list[float32] = [1.0, 3.0]
    print(fx_sum_f32(halves, 2))
    fx_halve(halves, 2)
    print(halves)
    mut flags: list[bool] = [true, false, false]
    print(fx_count_true(flags, 3))
    fx_flip(flags, 3)
    print(flags)
    print(fx_count_true(flags, 3))
    big: list[uint64] = [3, 18446744073709551615, 7]
    print(fx_max_u64(big, 3))

    a: list[int8] = [-1, -128]
    b: list[uint8] = [255, 1]
    c: list[uint16] = [65535, 2]
    d: list[uint32] = [4294967295, 3]
    mut wide: list[int64] = [0, 0]
    fx_widen_all(a, b, c, d, wide, 2)
    print(wide)

    empty: list[float64] = []
    print(fx_is_null(empty))
    print(fx_is_null(xs))
    mut none: list[int32] = []
    print(fx_is_null_mut(none))
    none.append(1)
    print(fx_is_null_mut(none))

    mut samples = Samples(values=[1.0, 2.0])
    fx_scale(samples.values, 2, 10.0)
    print(samples.values)
    print(fx_dot(samples.values, samples.values, 2))

    mut round = 0
    mut total = 0.0
    while round < 2000:
        fx_scale(zs, 3, 1.0)
        total += fx_dot(zs, ys, 3)
        round += 1
    print(total)
    return 0
"#,
    );
    let (mir, direct) = run_on_both_backends(&package, &source);
    assert_success(&mir, "forced MIR pointer fixture run");
    assert_success(&direct, "direct pointer fixture run");
    let expected = "\
32.0\n5.0\n3.0\n[2.5, 5.0, 7.5]\n[1.0, 2.0, 3.0]\n[5.0, 5.0, 7.5]\n\
328350\n14\n[9, 18, 27, 36]\n4.0\n[0.5, 1.5]\n1\n[false, true, true]\n2\n\
18446744073709551615\n[4295033084, -122]\n\
true\nfalse\ntrue\nfalse\n[10.0, 20.0]\n500.0\n180000.0\n";
    assert_eq!(String::from_utf8_lossy(&mir.stdout), expected);
    assert_eq!(String::from_utf8_lossy(&direct.stdout), expected);
}

#[test]
fn array_parameters_lend_their_elements_with_backend_parity() {
    let (package, source) = fixture_package(
        r#"
class Model:
    weights: Array[float64]

def main() -> int32:
    xs = Array[float64].full([3], 2.0)
    mut ys = Array[float64].zeros([3])
    ys[0] = 1.0
    ys[1] = 2.0
    ys[2] = 3.0
    print(ax_dot(xs, ys, 3))
    print(ax_dot(ys, ys, 2))

    # A write lands in the Array, and a clone keeps its elements.
    kept = ys.clone()
    ax_scale(ys, 3, 2.5)
    print(ys)
    print(kept)

    # A matrix crosses in row-major order.
    mut grid = Array[int32].zeros([2, 3])
    for row in range(2):
        for column in range(3):
            grid[row, column] = (row * 3 + column + 1) as int32
    print(ax_sum_i32(grid, 6))
    # A view is packed for a read: the columns, then a reversed row.
    print(ax_sum_i32(grid.transpose(), 6))
    print(ax_sum_i32(grid[1, ::-1], 3))
    print(ax_sum_i32(grid[:, 1], 2))

    # A written view becomes its own block and leaves its source alone.
    mut column = grid[:, 1] as Array[float64]
    mut picked = column[::-1]
    ax_scale(picked, 2, 10.0)
    print(picked)
    print(column)

    mut counts = Array[int64].zeros([2, 2])
    ax_count_i64(counts, 4, 9000000000)
    print(counts)
    print(counts[1, 1])
    mut halves = Array[float32].full([4], 3.0)
    ax_halve_f32(halves, 2)
    print(halves)

    empty = Array[float64].zeros([0])
    print(ax_is_null(empty))
    print(ax_is_null(xs))

    mut model = Model(weights=Array[float64].full([2], 1.5))
    ax_scale(model.weights, 2, 2.0)
    print(model.weights)
    print(ax_dot(model.weights, model.weights, 2))

    mut round = 0
    mut total = 0.0
    while round < 2000:
        ax_scale(ys, 3, 1.0)
        total += ax_dot(xs, ys, 3)
        round += 1
    print(total)
    return 0
"#,
    );
    let (mir, direct) = run_on_both_backends(&package, &source);
    assert_success(&mir, "forced MIR Array pointer fixture run");
    assert_success(&direct, "direct Array pointer fixture run");
    let expected = "\
12.0\n5.0\n\
Array[float64](shape=[3], values=[2.5, 5.0, 7.5])\n\
Array[float64](shape=[3], values=[1.0, 2.0, 3.0])\n\
123456\n142536\n654\n25\n\
Array[float64](shape=[2], values=[50.0, 20.0])\n\
Array[float64](shape=[2], values=[2.0, 5.0])\n\
Array[int64](shape=[2, 2], values=[9000000000, 9000000001, 9000000002, 9000000003])\n\
9000000003\n\
Array[float32](shape=[4], values=[1.5, 1.5, 3.0, 3.0])\n\
true\nfalse\n\
Array[float64](shape=[2], values=[3.0, 3.0])\n18.0\n60000.0\n";
    assert_eq!(String::from_utf8_lossy(&mir.stdout), expected);
    assert_eq!(String::from_utf8_lossy(&direct.stdout), expected);
}

#[test]
fn structs_cross_by_value_and_by_pointer_with_backend_parity() {
    // Seven floats leave one float register on AArch64, which a `Vec2`
    // does not fit. C passes it whole on the stack there. A built program
    // refuses the call, so that host runs the rest of the fixture and
    // checks the refusal by itself.
    let spill = "fx_spill_floats(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, b, 0.5)";
    let refused = cfg!(target_arch = "aarch64");
    let main = r#"
class Scene:
    origin: Vec2
    bounds: Rect

def length2(v: Vec2) -> float64:
    return fx_vec2_len2(v)

def main() -> int32:
    a = Vec2(x=3.0, y=4.0)
    b = Vec2(x=0.5, y=-1.0)
    print(fx_vec2_len2(a))
    print(length2(b))
    sum = fx_vec2_add(a, b)
    print(sum.x)
    print(sum.y)
    print(fx_vec2_len2(fx_vec2_add(sum, sum)))

    mut v = Vec2(x=1.5, y=-2.0)
    fx_vec2_scale(v, 4.0)
    print(v.x)
    print(v.y)
    mut w = a
    print(fx_vec2_dot(v, w))
    print(a.x)

    pair = fx_ipair_swap(IPair(a=-7, b=2000000000))
    print(pair.a)
    print(pair.b)
    print(fx_ipair_diff(pair))

    mixed = fx_mixed_bump(Mixed(n=41, w=0.25))
    print(mixed.n)
    print(mixed.w)
    print(fx_mixed_sum(mixed))

    cross = fx_vec3_cross(Vec3(x=1.0, y=0.0, z=0.0), Vec3(x=0.0, y=1.0, z=0.0))
    print(cross.x)
    print(cross.y)
    print(cross.z)
    print(fx_vec3_sum(Vec3(x=0.5, y=0.25, z=2.0)))

    tagged = fx_tagged_next(Tagged(tag=-128, v=-5))
    print(tagged.tag)
    print(tagged.v)

    triple = fx_triple_rotate(fx_triple_make(10))
    print(triple.a)
    print(triple.b)
    print(triple.c)
    print(fx_triple_sum(triple))

    mut rect = fx_rect_grow(Rect(lo=Vec2(x=0.0, y=0.0), hi=Vec2(x=2.0, y=3.0)), 1.0)
    print(fx_rect_area(rect))
    fx_rect_move(rect, Vec2(x=10.0, y=20.0))
    print(rect.lo.x)
    print(rect.lo.y)
    print(rect.hi.x)
    print(rect.hi.y)

    small = fx_small_make(3)
    print(small.on)
    print(small.b)
    print(small.s)
    print(small.w)
    print(fx_small_pack(small))
    print(fx_small_pack(Small(on=false, b=255, s=65535, w=4294967295)))

    nest = fx_nest_make(9, 0.5)
    print(nest.p.a)
    print(nest.p.b)
    print(nest.f)
    print(fx_nest_sum(nest))

    fi = fx_float_int(FloatInt(f=1.25, i=-2))
    print(fi.f)
    print(fi.i)

    rgb = fx_rgb_invert(Rgb(r=0, g=128, b=255))
    print(rgb.r)
    print(rgb.g)
    print(rgb.b)

    wide = Wide(lo=100, hi=-1000)
    print(fx_spill_ints(1, 2, 3, 4, 5, wide, 6))
    print(fx_spill_floats(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, b, 0.5))
    print(fx_mix_all(a, pair, mixed, triple, b, wide))
    from_parts = fx_triple_from(wide, a)
    print(from_parts.a)
    print(from_parts.b)
    print(from_parts.c)

    mut scene = Scene(origin=Vec2(x=1.0, y=1.0), bounds=rect)
    fx_vec2_scale(scene.origin, 3.0)
    fx_rect_move(scene.bounds, scene.origin)
    print(scene.origin.x)
    print(fx_rect_area(scene.bounds))
    print(scene.bounds.hi.y)

    points: list[Vec2] = [a, b, sum]
    mut total = 0.0
    for point in points:
        total += fx_vec2_len2(point)
    print(total)

    mut round = 0
    mut acc = Vec2(x=0.0, y=0.0)
    while round < 2000:
        acc = fx_vec2_add(acc, b)
        round += 1
    print(acc.x)
    print(acc.y)
    return 0
"#;
    if refused {
        let (package, source) = fixture_package(&format!(
            "def main() -> int32:\n    b = Vec2(x=0.5, y=-0.25)\n    print({spill})\n    return 0\n"
        ));
        assert_success(&run_mir(&source), "the spilled struct on the interpreter");
        let build = build_direct(&source, &package.path.join("ffi-fixture-spill"));
        let stderr = String::from_utf8_lossy(&build.stderr);
        assert!(!build.status.success(), "{stderr}");
        assert!(
            stderr.contains("struct `Vec2` does not fit the argument registers"),
            "{stderr}"
        );
    }
    let main = if refused {
        main.replace(spill, "433.0")
    } else {
        main.to_string()
    };
    let (package, source) = fixture_package(&main);
    let (mir, direct) = run_on_both_backends(&package, &source);
    assert_success(&mir, "structs on the interpreter");
    assert_success(&direct, "structs in a built program");
    let expected = "25.0\n1.25\n3.5\n3.0\n85.0\n6.0\n-8.0\n-14.0\n3.0\n\
        2000000000\n-7\n2000000007\n42\n0.5\n42.5\n0.0\n0.0\n1.0\n2.75\n\
        127\n-15\n11\n12\n10\n33\n20.0\n9.0\n19.0\n13.0\n24.0\n\
        true\n203\n65003\n3000000000\n129476203069332992\n72057594037927935\n\
        9\n-9\n0.5\n0.5\n2.5\n-1\n255\n127\n0\n-6297\n433.0\n1999999175.0\n\
        100\n-1000\n7\n3.0\n20.0\n27.0\n47.5\n1000.0\n-2000.0\n";
    assert_eq!(String::from_utf8_lossy(&mir.stdout), expected);
    assert_eq!(String::from_utf8_lossy(&direct.stdout), expected);
}

#[test]
fn callbacks_run_aura_functions_from_c_with_backend_parity() {
    let (package, source) = fixture_package(
        r#"
def add_square(total: int64, item: int32) -> int64:
    return total + ((item * item) as int64)

def parabola(x: float64) -> float64:
    return 3.0 * x * x

def show(a: uint8, b: int16, c: uint16, d: uint32, e: uint64, f: int8, g: float32, h: bool) -> None:
    print(f"{a} {b} {c} {d} {e} {f} {g} {h}")

def is_even(value: int32) -> bool:
    return value % 2 == 0

def advance(slot: mut int32, sum: mut float64, odd: mut bool) -> None:
    sum += slot as float64
    slot = slot * 2
    odd = not odd

def same_i8(v: int8) -> int8:
    return v

def same_u16(v: uint16) -> uint16:
    return v

def same_i16(v: int16) -> int16:
    return v

def same_u8(v: uint8) -> uint8:
    return v

def same_i32(v: int32) -> int32:
    return v

def same_u32(v: uint32) -> uint32:
    return v

def mix(a: float32, b: float32) -> float32:
    return a * b

def bump(v: uint64) -> uint64:
    return v + 615

def read_slot(slot: mut int32) -> int32:
    seen = slot
    slot = slot + 1
    return seen

def negate(v: bool) -> bool:
    return not v

def announce(value: int32) -> None:
    print(f"kept {value}")

def halve_with_c(value: int32) -> int32:
    return (fx_widen_i32(value) as int32) // 2

def nested(value: int32) -> int32:
    return fx_twice(halve_with_c, value) + 1

def descending(a: mut int32, b: mut int32) -> int32:
    if a < b:
        return 1
    if a > b:
        return -1
    return 0

def main() -> int32:
    print(fx_fold(add_square, 10))
    print(fx_integrate(parabola, 0.0, 2.0, 1000) > 7.99)
    fx_visit(show)
    values: list[int32] = [1, 2, 4, 7, 10, 12]
    print(fx_count_if(is_even, values, 6))
    print(fx_step_all(advance, 5))
    print(fx_narrow_results(same_i8, same_u16, same_i16, same_u8, same_i32, same_u32))
    print(fx_blend(mix))
    print(fx_big(bump))
    print(fx_null_slot(read_slot))
    print(fx_truthy(negate))
    fx_fire(1)
    fx_keep(announce)
    fx_fire(2)
    fx_fire(3)
    print(fx_twice(nested, 100))
    mut sorted: list[int32] = [3, 9, -4, 12, 0, 9]
    qsort(sorted, 6, 4, descending)
    print(sorted)
    mut round = 0
    mut total: int64 = 0
    while round < 2000:
        total += fx_fold(add_square, 3)
        round += 1
    print(total)
    return 0
"#,
    );
    let (mir, direct) = run_on_both_backends(&package, &source);
    assert_success(&mir, "callbacks on the interpreter");
    assert_success(&direct, "callbacks in a built program");
    let expected = "385\ntrue\n\
        255 -32768 65535 4294967295 18446744073709551615 -128 0.25 true\n\
        0 32767 0 7 9 127 -1.5 false\n\
        4\n3063.5\n2000035100\n1.375\n18446744073709551615\n11\nfalse\n\
        kept 2\nkept 3\n7\n[12, 9, 9, 3, 0, -4]\n28000\n";
    assert_eq!(String::from_utf8_lossy(&mir.stdout), expected);
    assert_eq!(String::from_utf8_lossy(&direct.stdout), expected);
}

#[test]
fn handles_with_a_drop_function_are_released_once_with_backend_parity() {
    let (package, source) = fixture_package(
        r#"
class Shelf:
    label: str
    held: Crate

def report(step: str) -> None:
    mut closed: list[int32] = []
    mut index: int32 = 0
    while index < fx_crate_closes():
        closed.append(fx_crate_closed(index))
        index = index + 1
    print(f"{step}: open {fx_crates_open()} closed {closed}")

def scoped() -> int32:
    held = fx_crate_open(1)
    return fx_crate_id(held)

def closed_by_hand() -> None:
    held = fx_crate_open(2)
    fx_crate_close(held)
    report("by hand, before return")

def taken(held: own Crate) -> int32:
    return fx_crate_id(held) + 1000

def in_a_class() -> str:
    shelf = Shelf(label="top", held=fx_crate_open(4))
    return f"{shelf.label} {fx_crate_id(shelf.held)}"

def in_a_list() -> int64:
    mut crates: list[Crate] = []
    crates.append(fx_crate_open(5))
    crates.append(fx_crate_open(6))
    crates.append(fx_crate_open(7))
    fx_crate_close(crates.pop())
    return len(crates)

def replaced() -> None:
    mut held = fx_crate_open(8)
    held = fx_crate_open(9)
    report("replaced, before return")
    print(fx_crate_id(held))

def found() -> None:
    match fx_crate_find(-1):
        case None:
            print("none")
        case Crate as held:
            print(fx_crate_id(held))
    match fx_crate_find(10):
        case None:
            print("none")
        case Crate as held:
            print(fx_crate_id(held))

def stored() -> None:
    mut slot: Crate | None = None
    print(fx_crate_make(11, slot))
    fx_crate_keep(slot)
    fx_crate_keep(slot)
    match slot:
        case None:
            print("none")
        case Crate as held:
            print(fx_crate_id(held))
    fx_crate_swap(slot, 12)
    report("swapped, before return")
    print(fx_crate_make(-1, slot))
    report("stored over, before return")

def ordered() -> int32:
    # The order among handles that go out of scope together is not
    # specified, so the three carry one id.
    first = fx_crate_open(22)
    second = fx_crate_open(22)
    third = fx_crate_open(22)
    total = fx_crate_id(third) + fx_crate_id(first)
    return total + fx_crate_id(second)

def failing(index: int64) -> int32:
    held = fx_crate_open(31)
    values: list[int32] = [fx_crate_id(held)]
    return values[index]

def main() -> int32:
    print(scoped())
    report("scoped")
    closed_by_hand()
    report("by hand")
    print(taken(fx_crate_open(3)))
    report("taken")
    print(in_a_class())
    report("class")
    print(in_a_list())
    report("list")
    replaced()
    report("replaced")
    found()
    report("found")
    stored()
    report("stored")
    print(ordered())
    report("ordered")
    print(failing(0))
    report("not failing")
    return 0
"#,
    );
    let (mir, direct) = run_on_both_backends(&package, &source);
    assert_success(&mir, "MIR handle drop fixture");
    assert_success(&direct, "direct handle drop fixture");
    // The last call stores a null pointer over the crate the place
    // held, so that crate is no longer the program's to release.
    let expected = "1\n\
scoped: open 0 closed [1]\n\
by hand, before return: open 0 closed [1, 2]\n\
by hand: open 0 closed [1, 2]\n\
1003\n\
taken: open 0 closed [1, 2, 3]\n\
top 4\n\
class: open 0 closed [1, 2, 3, 4]\n\
2\n\
list: open 0 closed [1, 2, 3, 4, 7, 5, 6]\n\
replaced, before return: open 1 closed [1, 2, 3, 4, 7, 5, 6, 8]\n\
9\n\
replaced: open 0 closed [1, 2, 3, 4, 7, 5, 6, 8, 9]\n\
none\n\
10\n\
found: open 0 closed [1, 2, 3, 4, 7, 5, 6, 8, 9, 10]\n\
0\n\
211\n\
swapped, before return: open 1 closed [1, 2, 3, 4, 7, 5, 6, 8, 9, 10, 211]\n\
1\n\
stored over, before return: open 1 closed [1, 2, 3, 4, 7, 5, 6, 8, 9, 10, 211]\n\
stored: open 1 closed [1, 2, 3, 4, 7, 5, 6, 8, 9, 10, 211]\n\
66\n\
ordered: open 1 closed [1, 2, 3, 4, 7, 5, 6, 8, 9, 10, 211, 22, 22, 22]\n\
31\n\
not failing: open 1 closed [1, 2, 3, 4, 7, 5, 6, 8, 9, 10, 211, 22, 22, 22, 31]\n\
";
    assert_eq!(String::from_utf8_lossy(&mir.stdout), expected);
    assert_eq!(String::from_utf8_lossy(&direct.stdout), expected);
}

#[test]
fn a_failing_callback_fails_its_foreign_call_with_backend_parity() {
    let (package, source) = fixture_package(
        r#"
def reciprocal(value: int32) -> int32:
    print(f"called with {value}")
    return 12 // value

def main() -> int32:
    print(fx_until_failure(reciprocal))
    print("not reached")
    return 0
"#,
    );
    let (mir, direct) = run_on_both_backends(&package, &source);
    for (backend, output) in [("mir", &mir), ("direct", &direct)] {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{backend}: {stdout}");
        assert!(
            stdout.starts_with("called with 3\ncalled with 2\ncalled with 1\ncalled with 0\n"),
            "{backend}: {stdout}"
        );
        assert!(!stdout.contains("called with -1"), "{backend}: {stdout}");
        assert!(!stdout.contains("not reached"), "{backend}: {stdout}");
        assert!(stderr.contains("AU4"), "{backend}: {stderr}");
        assert!(stderr.contains("division by zero"), "{backend}: {stderr}");
    }
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
        (
            "def main() -> int32:\n    mut flags: list[bool] = [true, false]\n    fx_bad_flags(flags)\n    print(flags)\n    return 0\n",
            "AU4001",
            "FFI call to `fx_bad_flags` failed",
        ),
        (
            "def main() -> int32:\n    flag = fx_bad_struct_flag()\n    print(flag.ok)\n    return 0\n",
            "AU4001",
            "FFI call to `fx_bad_struct_flag` failed",
        ),
        (
            "def main() -> int32:\n    mut flag = Flag(ok=true)\n    fx_bad_struct_out(flag)\n    print(flag.ok)\n    return 0\n",
            "AU4001",
            "FFI call to `fx_bad_struct_out` failed",
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

#[test]
fn extern_c_contracts_enforce_requires_and_ensures_at_runtime_on_both_backends() {
    let package = TempPackage::new(true);
    package.c_library("aurafixture", FIXTURE_LIBRARY);
    package.manifest("\n[ffi]\nlibraries = [\"aurafixture\"]\nlibrary_paths = [\"native\"]\n");

    // 1. Valid requires & ensures succeed on both backends.
    let ok_source = package.source(
        r#"extern "C" def fx_i32(v: int64) -> int32:
    requires v >= 0 and v <= 100
    ensures result == v as int32

def main() -> int32:
    print(fx_i32(42))
    return 0
"#,
    );
    let (mir_ok, direct_ok) = run_on_both_backends(&package, &ok_source);
    assert_success(&mir_ok, "MIR valid extern contract");
    assert_success(&direct_ok, "direct valid extern contract");
    assert_eq!(mir_ok.stdout, b"42\n");
    assert_eq!(direct_ok.stdout, b"42\n");

    // 2. Violated `requires` traps with AU4009 on both backends.
    let bad_req = package.source(
        r#"extern "C" def fx_i32(v: int64) -> int32:
    requires v >= 0

def main() -> int32:
    print(fx_i32(-5))
    return 0
"#,
    );
    let (mir_req, direct_req) = run_on_both_backends(&package, &bad_req);
    for (name, out) in [("mir", mir_req), ("direct", direct_req)] {
        assert!(
            !out.status.success(),
            "{name} must reject violated extern requires"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("AU4009"),
            "{name} expected AU4009, got: {stderr}"
        );
    }

    // 3. Violated `ensures` traps with AU4009 on both backends AND under `--remove-proved-checks`
    // before any downstream proved unchecked index can read out-of-bounds memory.
    let bad_ensures = package.source(
        r#"extern "C" def fx_i32(v: int64) -> int32:
    ensures 0 <= result and result < 2

def main() -> int32:
    xs = [10, 20]
    idx = fx_i32(99) as int64
    print(xs[idx])
    return 0
"#,
    );
    let (mir_ens, direct_ens) = run_on_both_backends(&package, &bad_ensures);
    for (name, out) in [("mir", mir_ens), ("direct", direct_ens)] {
        assert!(
            !out.status.success(),
            "{name} must reject violated extern ensures"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("AU4009"),
            "{name} expected AU4009, got: {stderr}"
        );
    }

    let solver_available = ["z3", "cvc5"].iter().any(|name| {
        std::env::var_os("PATH")
            .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
    }) || std::env::var_os("AURA_SMT_SOLVER").is_some();
    if solver_available {
        let proved_bin = package.path.join("ffi-bad-ensures-proved");
        let build = Command::new(aura_bin())
            .args([
                "build",
                "--backend",
                "direct",
                "--remove-proved-checks",
                "-o",
            ])
            .arg(&proved_bin)
            .arg(&bad_ensures)
            .output()
            .expect("aura build --remove-proved-checks should start");
        assert_success(&build, "direct FFI --remove-proved-checks build");
        let proved_run = Command::new(&proved_bin)
            .current_dir(std::env::temp_dir())
            .output()
            .expect("direct FFI --remove-proved-checks binary should start");
        assert!(
            !proved_run.status.success(),
            "--remove-proved-checks binary must trap on violated extern ensures"
        );
        let stderr = String::from_utf8_lossy(&proved_run.stderr);
        assert!(
            stderr.contains("AU4009"),
            "--remove-proved-checks expected AU4009, got: {stderr}"
        );
    }
}

#[test]
fn contracts_wrap_every_foreign_parameter_kind_with_backend_parity() {
    let package = TempPackage::new(true);
    let source = package.source(
        r#"extern "C" class Div:
    quot: int32
    rem: int32

extern "C" def strlen(text: cstr) -> uint64:
    ensures result >= 0
extern "C" def frexp(value: float64, exponent: mut int32) -> float64:
    requires value > 0.0
extern "C" def div(numerator: int32, denominator: int32) -> Div:
    requires denominator != 0
extern "C" def qsort(
    base: mut ptr[int32], count: uint64, size: uint64,
    compare: def(a: mut int32, b: mut int32) -> int32
) -> None:
    requires size == 4

def descending(a: mut int32, b: mut int32) -> int32:
    if a < b:
        return 1
    if a > b:
        return -1
    return 0

def main() -> int32:
    print(strlen("hello"))
    mut exponent: int32 = 0
    print(frexp(48.0, exponent))
    print(exponent)
    d = div(17, 5)
    print(d.quot)
    print(d.rem)
    mut values: list[int32] = [3, 9, 1, 7, 5]
    qsort(values, 5, 4, descending)
    print(values)
    zero: int32 = d.rem - 2
    print(div(1, zero).quot)
    return 0
"#,
    );
    let (mir, direct) = run_on_both_backends(&package, &source);
    for (name, out) in [("mir", mir), ("direct", direct)] {
        assert!(!out.status.success(), "{name} must stop at the last call");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "5\n0.75\n6\n3\n2\n[9, 7, 5, 3, 1]\n",
            "{name}"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("AU4009") && stderr.contains("denominator != 0"),
            "{name}: {stderr}"
        );
    }
}
