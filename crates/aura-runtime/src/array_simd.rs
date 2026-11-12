//! Array kernels that run at the widest vector width the CPU has
//! (ADR-0085), and the kernels ADR-0086 adds: reordered reductions, `dot`,
//! and `matmul`.
//!
//! Every kernel here is ordinary Rust over slices that LLVM vectorizes.
//! [`widest`] runs a kernel in a copy compiled for AVX2 when the CPU has
//! it, and in the baseline copy otherwise. A kernel never uses fused
//! multiply-add and never lets the vector width change the order of its
//! floating-point operations, so every CPU computes the same bits.
//!
//! A reordered float reduction adds in one fixed order. Element `i` goes to
//! partial sum `i % STRIPES`, in index order, for the elements before the
//! last partial group of `STRIPES`. The partial sums are then added in
//! halves: sum `s` takes sum `s + 16`, then `s + 8`, `s + 4`, `s + 2`, and
//! `s + 1`. The elements of the last partial group are added to that
//! total in index order. Sixteen 128-bit registers, eight 256-bit
//! registers, or four 512-bit registers hold the 32 `float64` partial sums,
//! so each width runs the same additions.

use std::sync::atomic::{AtomicU8, Ordering};

/// The partial sums of a reordered float reduction.
pub const STRIPES: usize = 32;

/// The widest instruction set the Array kernels use on this CPU.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum KernelLevel {
    /// The target's baseline: SSE2 on x86-64, NEON on arm64.
    Baseline = 1,
    /// 256-bit AVX2 on x86-64.
    Avx2 = 2,
}

impl KernelLevel {
    /// The name `aura` commands print, and `AURA_ARRAY_KERNELS` accepts.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Avx2 => "avx2",
        }
    }
}

static DETECTED_LEVEL: AtomicU8 = AtomicU8::new(0);

thread_local! {
    static LEVEL_OVERRIDE: std::cell::Cell<Option<KernelLevel>> =
        const { std::cell::Cell::new(None) };
}

/// Whether the CPU runs AVX2 code.
fn cpu_has_avx2() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// The level for a CPU that has AVX2 or not, under the `AURA_ARRAY_KERNELS`
/// setting `requested`. `baseline` turns the wider copies off. Any other
/// setting, and none, uses what the CPU has.
fn level_for(has_avx2: bool, requested: Option<&str>) -> KernelLevel {
    if has_avx2 && requested != Some("baseline") {
        KernelLevel::Avx2
    } else {
        KernelLevel::Baseline
    }
}

/// The level this thread's kernels run at. The process reads the CPU and
/// the `AURA_ARRAY_KERNELS` environment variable once.
pub fn kernel_level() -> KernelLevel {
    if let Some(level) = LEVEL_OVERRIDE.with(std::cell::Cell::get) {
        return level;
    }
    match DETECTED_LEVEL.load(Ordering::Relaxed) {
        1 => KernelLevel::Baseline,
        2 => KernelLevel::Avx2,
        _ => {
            let requested = std::env::var("AURA_ARRAY_KERNELS").ok();
            let level = level_for(cpu_has_avx2(), requested.as_deref());
            DETECTED_LEVEL.store(level as u8, Ordering::Relaxed);
            level
        }
    }
}

/// Runs `operation` with this thread's kernels at `level`, or at the
/// baseline when the CPU does not have `level`. Tests compare the copies
/// of one kernel through it.
pub fn with_kernel_level<T>(level: KernelLevel, operation: impl FnOnce() -> T) -> T {
    struct Restore(Option<KernelLevel>);
    impl Drop for Restore {
        fn drop(&mut self) {
            LEVEL_OVERRIDE.with(|level| level.set(self.0));
        }
    }
    let level = level_for(
        cpu_has_avx2(),
        (level == KernelLevel::Baseline).then_some("baseline"),
    );
    let _restore = Restore(LEVEL_OVERRIDE.with(|current| current.replace(Some(level))));
    operation()
}

/// Runs `kernel` in the copy compiled for the widest instruction set the
/// CPU has. The kernel and every function it calls in its loops must be
/// `#[inline(always)]`, so the whole loop lands in the copy.
#[inline(always)]
pub(crate) fn widest<R>(kernel: impl FnOnce() -> R) -> R {
    #[cfg(target_arch = "x86_64")]
    if kernel_level() == KernelLevel::Avx2 {
        // SAFETY: `kernel_level` returns `Avx2` only when the CPU has it.
        return unsafe { widest_avx2(kernel) };
    }
    kernel()
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn widest_avx2<R>(kernel: impl FnOnce() -> R) -> R {
    kernel()
}

/// The float element types of an Array.
pub(crate) trait KernelFloat:
    Copy
    + Default
    + PartialOrd
    + Send
    + Sync
    + std::ops::Add<Output = Self>
    + std::ops::Mul<Output = Self>
    + std::ops::AddAssign
{
    fn to_f64(self) -> f64;
    fn is_nan_value(self) -> bool;
    /// Whether the value is a zero with its sign bit set.
    fn is_negative_zero(self) -> bool;
    /// Whether the value is a zero with its sign bit clear.
    fn is_positive_zero(self) -> bool;
}

macro_rules! kernel_float {
    ($($ty:ty),*) => {$(
        impl KernelFloat for $ty {
            #[inline(always)]
            fn to_f64(self) -> f64 {
                self as f64
            }

            #[inline(always)]
            fn is_nan_value(self) -> bool {
                self.is_nan()
            }

            #[inline(always)]
            fn is_negative_zero(self) -> bool {
                self.to_bits() == (-0.0 as $ty).to_bits()
            }

            #[inline(always)]
            fn is_positive_zero(self) -> bool {
                self.to_bits() == 0
            }
        }
    )*};
}

kernel_float!(f32, f64);

/// Adds the partial sums in halves and returns the total.
#[inline(always)]
fn fold_stripes<T: Copy + std::ops::AddAssign>(mut partial: [T; STRIPES]) -> T {
    let mut width = STRIPES;
    while width > 1 {
        width /= 2;
        for stripe in 0..width {
            let upper = partial[stripe + width];
            partial[stripe] += upper;
        }
    }
    partial[0]
}

/// The reordered sum of `term(left[i], right[i])` over two slices of one
/// length, in the order the module comment gives.
#[inline(always)]
fn striped_total<A: Copy, B: Copy, T: Copy + Default + std::ops::AddAssign>(
    left: &[A],
    right: &[B],
    term: impl Fn(A, B) -> T,
) -> T {
    let (left_groups, left_tail) = left.as_chunks::<STRIPES>();
    let (right_groups, right_tail) = right.as_chunks::<STRIPES>();
    let mut partial = [T::default(); STRIPES];
    for (lhs, rhs) in left_groups.iter().zip(right_groups) {
        for stripe in 0..STRIPES {
            partial[stripe] += term(lhs[stripe], rhs[stripe]);
        }
    }
    let mut total = fold_stripes(partial);
    for (&lhs, &rhs) in left_tail.iter().zip(right_tail) {
        total += term(lhs, rhs);
    }
    total
}

/// The reordered sum of a float Array's elements, in their own type.
pub(crate) fn striped_sum<T: KernelFloat>(values: &[T]) -> T {
    widest(
        #[inline(always)]
        || striped_total(values, values, |value, _| value),
    )
}

/// The reordered sum of `count` elements of `values` that lie `step`
/// apart from `position`, with the bits `striped_sum` gives for the same
/// elements in one slice.
pub(crate) fn striped_sum_strided<T: KernelFloat>(
    values: &[T],
    position: usize,
    step: isize,
    count: usize,
) -> T {
    let Some(last) = count.checked_sub(1) else {
        return T::default();
    };
    // Both ends are checked once, so the loops below read unchecked.
    let end = position as isize + last as isize * step;
    assert!(position < values.len() && (0..values.len() as isize).contains(&end));
    let element = |index: usize| {
        // SAFETY: every index is below `count`, and the first and last
        // elements were checked to lie inside `values`.
        unsafe { *values.get_unchecked((position as isize + index as isize * step) as usize) }
    };
    widest(
        #[inline(always)]
        || {
            let groups = count / STRIPES;
            let mut partial = [T::default(); STRIPES];
            for group in 0..groups {
                for (stripe, slot) in partial.iter_mut().enumerate() {
                    *slot += element(group * STRIPES + stripe);
                }
            }
            let mut total = fold_stripes(partial);
            for index in groups * STRIPES..count {
                total += element(index);
            }
            total
        },
    )
}

/// The reordered sum of a float Array's elements, each widened to
/// `float64` first.
pub(crate) fn striped_sum_f64<T: KernelFloat>(values: &[T]) -> f64 {
    widest(
        #[inline(always)]
        || striped_total(values, values, |value, _| value.to_f64()),
    )
}

/// The reordered sum of an `int32` Array's elements as `float64` values.
pub(crate) fn striped_sum_i32_as_f64(values: &[i32]) -> f64 {
    widest(
        #[inline(always)]
        || striped_total(values, values, |value, _| value as f64),
    )
}

/// The reordered sum of an `int64` Array's elements as `float64` values.
pub(crate) fn striped_sum_i64_as_f64(values: &[i64]) -> f64 {
    widest(
        #[inline(always)]
        || striped_total(values, values, |value, _| value as f64),
    )
}

/// The reordered sum of the products of two float slices of one length.
/// Each product is rounded before it is added.
pub(crate) fn striped_dot<T: KernelFloat>(left: &[T], right: &[T]) -> T {
    let right = &right[..left.len()];
    widest(
        #[inline(always)]
        || striped_total(left, right, |lhs, rhs| lhs * rhs),
    )
}

/// The least element of a non-empty float slice, or `None` when one is
/// NaN. A negative zero is below a positive zero.
pub(crate) fn float_min<T: KernelFloat>(values: &[T]) -> Option<T> {
    let (least, nan) = widest(
        #[inline(always)]
        || float_extreme(values, |candidate, best| candidate < best),
    );
    if nan {
        return None;
    }
    if least.is_positive_zero() && values.iter().any(|value| value.is_negative_zero()) {
        return values
            .iter()
            .copied()
            .find(|value| value.is_negative_zero());
    }
    Some(least)
}

/// The greatest element of a non-empty float slice, or `None` when one is
/// NaN. A positive zero is above a negative zero.
pub(crate) fn float_max<T: KernelFloat>(values: &[T]) -> Option<T> {
    let (greatest, nan) = widest(
        #[inline(always)]
        || float_extreme(values, |candidate, best| candidate > best),
    );
    if nan {
        return None;
    }
    if greatest.is_negative_zero() && values.iter().any(|value| value.is_positive_zero()) {
        return values
            .iter()
            .copied()
            .find(|value| value.is_positive_zero());
    }
    Some(greatest)
}

/// The element of a non-empty slice that `better` prefers to every other,
/// and whether any element is NaN. Zeros of either sign compare equal, so
/// the result may be a zero of either sign when both occur.
#[inline(always)]
fn float_extreme<T: KernelFloat>(values: &[T], better: impl Fn(T, T) -> bool) -> (T, bool) {
    const LANES: usize = 16;
    let mut best = [values[0]; LANES];
    let mut nan = [false; LANES];
    let whole = values.len() - values.len() % LANES;
    for group in values[..whole].chunks_exact(LANES) {
        for lane in 0..LANES {
            let candidate = group[lane];
            nan[lane] |= candidate.is_nan_value();
            if better(candidate, best[lane]) {
                best[lane] = candidate;
            }
        }
    }
    let mut found = best[0];
    let mut any_nan = nan[0];
    for lane in 1..LANES {
        any_nan |= nan[lane];
        if better(best[lane], found) {
            found = best[lane];
        }
    }
    for &candidate in &values[whole..] {
        any_nan |= candidate.is_nan_value();
        if better(candidate, found) {
            found = candidate;
        }
    }
    (found, any_nan)
}

/// The integer element types of an Array, as the product kernels use them.
pub(crate) trait KernelInteger: Copy + Default + Ord + Into<i128> {
    const MIN: i128;
    const MAX: i128;
    fn wrapping_sum(self, rhs: Self) -> Self;
    fn wrapping_difference(self, rhs: Self) -> Self;
    /// The wrapped product, and a mask that is negative when the exact
    /// product overflowed.
    fn product_overflow(self, rhs: Self) -> (Self, Self);
    /// The wrapped sum, and a mask that is negative when the exact sum
    /// overflowed.
    fn sum_overflow(self, rhs: Self) -> (Self, Self);
    fn mask_or(self, rhs: Self) -> Self;
    fn mask_is_set(self) -> bool;
    fn from_i128(value: i128) -> Self;
}

macro_rules! kernel_integer {
    ($($ty:ty),*) => {$(
        impl KernelInteger for $ty {
            const MIN: i128 = <$ty>::MIN as i128;
            const MAX: i128 = <$ty>::MAX as i128;

            #[inline(always)]
            fn wrapping_sum(self, rhs: Self) -> Self {
                self.wrapping_add(rhs)
            }

            #[inline(always)]
            fn wrapping_difference(self, rhs: Self) -> Self {
                self.wrapping_sub(rhs)
            }

            #[inline(always)]
            fn product_overflow(self, rhs: Self) -> (Self, Self) {
                let (product, overflow) = self.overflowing_mul(rhs);
                (product, -(overflow as Self))
            }

            #[inline(always)]
            fn sum_overflow(self, rhs: Self) -> (Self, Self) {
                let sum = self.wrapping_add(rhs);
                (sum, (self ^ sum) & (rhs ^ sum))
            }

            #[inline(always)]
            fn mask_or(self, rhs: Self) -> Self {
                self | rhs
            }

            #[inline(always)]
            fn mask_is_set(self) -> bool {
                self < 0
            }

            #[inline(always)]
            fn from_i128(value: i128) -> Self {
                value as Self
            }
        }
    )*};
}

kernel_integer!(i32, i64);

/// Elements per block of the checked integer dot product.
const DOT_BLOCK: usize = 256;

/// The checked dot product of two integer slices of one length, or the
/// flat index of the first product, or the first running sum in index
/// order, that leaves the element type's range.
///
/// A block whose products all fit, and whose running sum cannot leave the
/// range given its largest product, is added in one vectorized pass. Any
/// other block is added element by element to find the first failure.
pub(crate) fn checked_integer_dot<T: KernelInteger>(
    left: &[T],
    right: &[T],
) -> std::result::Result<T, usize> {
    let right = &right[..left.len()];
    widest(
        #[inline(always)]
        || {
            let mut sum = 0_i128;
            for (block_index, (lhs, rhs)) in left
                .chunks(DOT_BLOCK)
                .zip(right.chunks(DOT_BLOCK))
                .enumerate()
            {
                let mut overflow = T::default();
                let mut low = T::default();
                let mut high = T::default();
                let mut block_sum = T::default();
                for (&l, &r) in lhs.iter().zip(rhs) {
                    let (product, mask) = l.product_overflow(r);
                    overflow = overflow.mask_or(mask);
                    low = low.min(product);
                    high = high.max(product);
                    block_sum = block_sum.wrapping_sum(product);
                }
                let largest = Into::<i128>::into(low)
                    .abs()
                    .max(Into::<i128>::into(high).abs());
                let bound = largest * lhs.len() as i128;
                if !overflow.mask_is_set() && sum - bound >= T::MIN && sum + bound <= T::MAX {
                    // The block's exact sum is within `bound` of zero, so
                    // the wrapped sum is the exact one.
                    sum += Into::<i128>::into(block_sum);
                    continue;
                }
                for (offset, (&l, &r)) in lhs.iter().zip(rhs).enumerate() {
                    let product = Into::<i128>::into(l) * Into::<i128>::into(r);
                    sum += product;
                    if product < T::MIN || product > T::MAX || sum < T::MIN || sum > T::MAX {
                        return Err(block_index * DOT_BLOCK + offset);
                    }
                }
            }
            Ok(T::from_i128(sum))
        },
    )
}

/// Columns per block of a matrix product, so one block of the right
/// operand's rows stays in the cache while every row of the left operand
/// passes over it.
const MATMUL_COLUMN_BLOCK: usize = 512;
/// Rows of the right operand per block of a matrix product.
const MATMUL_ROW_BLOCK: usize = 64;

/// Inner indexes per block of a tiled matrix product. One block of a
/// tile's columns of the right operand, copied side by side, is 16 KiB
/// and stays in the first-level cache while every row passes over it.
const MATMUL_INNER_BLOCK: usize = 128;
/// Output rows one tile of a matrix product holds in registers.
const MATMUL_TILE_ROWS: usize = 2;
/// The multiply-adds a thread of a matrix product should have to itself.
/// Below it the product costs less than starting the thread.
const MATMUL_THREAD_WORK: usize = 1 << 21;
/// The most threads a matrix product runs on.
const MATMUL_MAX_THREADS: usize = 16;

/// The threads the Array kernels may use: `AURA_ARRAY_THREADS` when it is
/// a positive number, and otherwise the CPUs the process may run on, up
/// to [`MATMUL_MAX_THREADS`].
fn array_thread_limit() -> usize {
    static LIMIT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *LIMIT.get_or_init(|| {
        thread_limit_for(
            std::env::var("AURA_ARRAY_THREADS").ok().as_deref(),
            std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
        )
    })
}

/// The thread limit for the `AURA_ARRAY_THREADS` setting `requested` on a
/// machine with `available` CPUs.
fn thread_limit_for(requested: Option<&str>, available: usize) -> usize {
    requested
        .and_then(|text| text.trim().parse::<usize>().ok())
        .filter(|threads| *threads > 0)
        .unwrap_or(available)
        .min(MATMUL_MAX_THREADS)
}

/// How many threads a `rows` by `inner` by `columns` product runs on when
/// at most `limit` are allowed: one for each [`MATMUL_THREAD_WORK`]
/// multiply-adds, and never more than one for each tile of rows.
fn matmul_threads(rows: usize, inner: usize, columns: usize, limit: usize) -> usize {
    let work = rows.saturating_mul(inner).saturating_mul(columns);
    (work / MATMUL_THREAD_WORK)
        .min(rows / MATMUL_TILE_ROWS)
        .min(limit)
        .max(1)
}

/// `output += left * right` for a `rows` by `inner` left operand and an
/// `inner` by `columns` right operand, all row-major, where `output`
/// starts at zero. Each output element adds its `inner` products in
/// ascending order of the inner index, each rounded before it is added.
///
/// A large product splits its rows over threads. Every output element is
/// computed by one thread in that same order, so the result does not
/// depend on how many threads ran.
pub(crate) fn float_matmul<T: KernelFloat>(
    left: &[T],
    right: &[T],
    output: &mut [T],
    rows: usize,
    inner: usize,
    columns: usize,
) {
    let threads = matmul_threads(rows, inner, columns, array_thread_limit());
    float_matmul_on(threads, left, right, output, rows, inner, columns);
}

/// [`float_matmul`] on `threads` threads, the calling one among them.
pub(crate) fn float_matmul_on<T: KernelFloat>(
    threads: usize,
    left: &[T],
    right: &[T],
    output: &mut [T],
    rows: usize,
    inner: usize,
    columns: usize,
) {
    // The tiles pay with 256-bit registers. At the baseline the row
    // kernel is as fast, and other targets are not measured.
    let level = kernel_level();
    let tiled = level == KernelLevel::Avx2;
    if threads <= 1 || rows < 2 || inner == 0 || columns == 0 {
        return float_matmul_rows(tiled, left, right, output, rows, inner, columns);
    }
    // Each thread takes a run of whole tiles of rows.
    let share = rows.div_ceil(threads).next_multiple_of(MATMUL_TILE_ROWS);
    std::thread::scope(|scope| {
        let mut parts = left
            .chunks(share * inner)
            .zip(output.chunks_mut(share * columns));
        let first = parts.next();
        for (left, output) in parts {
            scope.spawn(move || {
                with_kernel_level(level, || {
                    let rows = output.len() / columns;
                    float_matmul_rows(tiled, left, right, output, rows, inner, columns);
                });
            });
        }
        if let Some((left, output)) = first {
            let rows = output.len() / columns;
            float_matmul_rows(tiled, left, right, output, rows, inner, columns);
        }
    });
}

/// [`float_matmul`] on the calling thread, in tiles where `tiled` says
/// so and a row at a time otherwise.
fn float_matmul_rows<T: KernelFloat>(
    tiled: bool,
    left: &[T],
    right: &[T],
    output: &mut [T],
    rows: usize,
    inner: usize,
    columns: usize,
) {
    widest(
        #[inline(always)]
        || {
            let covered = match (tiled, std::mem::size_of::<T>()) {
                (false, _) => 0,
                (true, 4) => float_matmul_tiles::<T, 32>(left, right, output, rows, inner, columns),
                (true, _) => float_matmul_tiles::<T, 16>(left, right, output, rows, inner, columns),
            };
            float_matmul_by_row(left, right, output, rows, inner, columns, covered);
        },
    )
}

/// The columns of a matrix product from `first_column` on, one output row
/// at a time: each pass adds four rows of the right operand, scaled, to
/// the row.
#[inline(always)]
fn float_matmul_by_row<T: KernelFloat>(
    left: &[T],
    right: &[T],
    output: &mut [T],
    rows: usize,
    inner: usize,
    columns: usize,
    first_column: usize,
) {
    for column_start in (first_column..columns).step_by(MATMUL_COLUMN_BLOCK) {
        let width = MATMUL_COLUMN_BLOCK.min(columns - column_start);
        for inner_start in (0..inner).step_by(MATMUL_ROW_BLOCK) {
            let inner_end = (inner_start + MATMUL_ROW_BLOCK).min(inner);
            for row in 0..rows {
                let target = &mut output[row * columns + column_start..][..width];
                let scales = &left[row * inner..][..inner];
                let mut k = inner_start;
                while k + 4 <= inner_end {
                    let (a0, a1, a2, a3) = (scales[k], scales[k + 1], scales[k + 2], scales[k + 3]);
                    let b0 = &right[k * columns + column_start..][..width];
                    let b1 = &right[(k + 1) * columns + column_start..][..width];
                    let b2 = &right[(k + 2) * columns + column_start..][..width];
                    let b3 = &right[(k + 3) * columns + column_start..][..width];
                    for j in 0..width {
                        target[j] =
                            (((target[j] + a0 * b0[j]) + a1 * b1[j]) + a2 * b2[j]) + a3 * b3[j];
                    }
                    k += 4;
                }
                while k < inner_end {
                    let scale = scales[k];
                    let source = &right[k * columns + column_start..][..width];
                    for j in 0..width {
                        target[j] += scale * source[j];
                    }
                    k += 1;
                }
            }
        }
    }
}

/// The whole tiles of `W` columns of a matrix product. Returns how many
/// columns they cover.
///
/// A tile is `R` output rows by `W` columns that stay in registers while
/// the inner index runs over a block. For each block the tile's columns of
/// the right operand are copied side by side into `panel`, so every row
/// group reads them in one pass. An element still adds its products in
/// ascending order of the inner index: the blocks are taken in order, and
/// a tile starts each block from the sums the last one stored.
#[inline(always)]
fn float_matmul_tiles<T: KernelFloat, const W: usize>(
    left: &[T],
    right: &[T],
    output: &mut [T],
    rows: usize,
    inner: usize,
    columns: usize,
) -> usize {
    let covered = columns - columns % W;
    if covered == 0 || rows == 0 || inner == 0 {
        return 0;
    }
    let mut panel = Vec::with_capacity(MATMUL_INNER_BLOCK.min(inner) * W);
    for column in (0..covered).step_by(W) {
        for first in (0..inner).step_by(MATMUL_INNER_BLOCK) {
            let last = (first + MATMUL_INNER_BLOCK).min(inner);
            panel.clear();
            for k in first..last {
                panel.extend_from_slice(&right[k * columns + column..][..W]);
            }
            let mut row = 0;
            while row + MATMUL_TILE_ROWS <= rows {
                float_matmul_tile::<T, MATMUL_TILE_ROWS, W>(
                    left, &panel, output, row, inner, columns, column, first,
                );
                row += MATMUL_TILE_ROWS;
            }
            while row < rows {
                float_matmul_tile::<T, 1, W>(
                    left, &panel, output, row, inner, columns, column, first,
                );
                row += 1;
            }
        }
    }
    covered
}

/// Adds one block of products to the `R` rows from `row` and the `W`
/// columns from `column` of the output. `panel` holds those columns of
/// the right operand for the inner indexes from `first`, one row after
/// another.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn float_matmul_tile<T: KernelFloat, const R: usize, const W: usize>(
    left: &[T],
    panel: &[T],
    output: &mut [T],
    row: usize,
    inner: usize,
    columns: usize,
    column: usize,
    first: usize,
) {
    let mut sums = [[T::default(); W]; R];
    let mut scales = [&left[..0]; R];
    for r in 0..R {
        sums[r].copy_from_slice(&output[(row + r) * columns + column..][..W]);
        scales[r] = &left[(row + r) * inner + first..][..panel.len() / W];
    }
    for (k, source) in panel.chunks_exact(W).enumerate() {
        for r in 0..R {
            let scale = scales[r][k];
            for j in 0..W {
                sums[r][j] += scale * source[j];
            }
        }
    }
    for r in 0..R {
        output[(row + r) * columns + column..][..W].copy_from_slice(&sums[r]);
    }
}

/// `output[row] = left[row, ..] . right` for a `rows` by `inner` matrix and
/// a vector of `inner` elements. Each output element adds its products in
/// ascending order of the inner index, as [`float_matmul`] does. Four rows
/// run side by side.
pub(crate) fn float_matrix_vector<T: KernelFloat>(
    left: &[T],
    right: &[T],
    output: &mut [T],
    rows: usize,
    inner: usize,
) {
    let right = &right[..inner];
    widest(
        #[inline(always)]
        || {
            let mut row = 0;
            while row + 4 <= rows {
                let r0 = &left[row * inner..][..inner];
                let r1 = &left[(row + 1) * inner..][..inner];
                let r2 = &left[(row + 2) * inner..][..inner];
                let r3 = &left[(row + 3) * inner..][..inner];
                let mut sums = [T::default(); 4];
                for k in 0..inner {
                    let factor = right[k];
                    sums[0] += r0[k] * factor;
                    sums[1] += r1[k] * factor;
                    sums[2] += r2[k] * factor;
                    sums[3] += r3[k] * factor;
                }
                output[row..row + 4].copy_from_slice(&sums);
                row += 4;
            }
            while row < rows {
                let values = &left[row * inner..][..inner];
                let mut sum = T::default();
                for k in 0..inner {
                    sum += values[k] * right[k];
                }
                output[row] = sum;
                row += 1;
            }
        },
    )
}

/// The checked integer form of [`float_matmul`]. Returns the flat index of
/// an output element whose product or running sum left the element type's
/// range.
pub(crate) fn integer_matmul<T: KernelInteger>(
    left: &[T],
    right: &[T],
    output: &mut [T],
    rows: usize,
    inner: usize,
    columns: usize,
) -> std::result::Result<(), usize> {
    widest(
        #[inline(always)]
        || {
            for row in 0..rows {
                let target = &mut output[row * columns..][..columns];
                for k in 0..inner {
                    let scale = left[row * inner + k];
                    let source = &right[k * columns..][..columns];
                    let mut overflow = T::default();
                    for j in 0..columns {
                        let (product, product_mask) = scale.product_overflow(source[j]);
                        let (sum, sum_mask) = target[j].sum_overflow(product);
                        target[j] = sum;
                        overflow = overflow.mask_or(product_mask).mask_or(sum_mask);
                    }
                    if overflow.mask_is_set() {
                        // Each sum wrapped, so the sum before this pass is
                        // the wrapped difference.
                        let column = (0..columns)
                            .find(|&j| {
                                let (product, product_mask) = scale.product_overflow(source[j]);
                                let before = target[j].wrapping_difference(product);
                                product_mask.mask_is_set()
                                    || before.sum_overflow(product).1.mask_is_set()
                            })
                            .unwrap_or(0);
                        return Err(row * columns + column);
                    }
                }
            }
            Ok(())
        },
    )
}

#[cfg(test)]
#[path = "array_simd_tests.rs"]
mod tests;
