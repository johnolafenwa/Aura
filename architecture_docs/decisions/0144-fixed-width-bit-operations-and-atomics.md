# 0144. Fixed-Width Bit Operations and Atomics

## Decision

Expose rotate, population count, leading and trailing zero count, and byte swap on all fixed-width integer receivers. A rotation accepts a nonnegative `int64` count modulo the receiver width. The three counts return `int64` and preserve the width of zero in their result.

Store `int128` and `uint128` in two words in freestanding functions. Pass the low word first across internal calls. Exports keep the existing portable scalar C ABI. Compile checked arithmetic and exact casts into the object. The wide divider uses unsigned restoring division and adjusts the signed quotient and remainder to match Aura floor division.

Give every unsigned atomic memory operation the widths 8, 16, 32 and 64. Interpret a missing ordering as `"seq_cst"`. Check string literals against the operation at its call site. The native emitter lowers the chosen ordering directly to LLVM. The interpreter uses the same order on a Rust atomic of the matching width. Compare-exchange takes `order` and `failure_order`. A missing failure ordering removes the release part of the success ordering.

## Reason

Fixed-width operations describe register and file layouts directly. An unsigned atomic's width controls which neighboring bytes an update can share with ordinary storage. Acquire and release pair a published value with its data without imposing a total order on unrelated cells. Literal orderings let every target emit a fixed instruction sequence. The existing FFI permission and alignment requirements apply.

## Evidence

The bit-method fixture checks both back ends, including signed receivers, large counts and zero. Freestanding fixtures check all three targets for undocumented undefined symbols. A hosted C harness exercises the same code against real atomic storage.
