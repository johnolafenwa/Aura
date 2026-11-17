# Collections, Text, And Numbers Design Checkpoint

Status: **Step 1 accepted 2026-11-16 as ADR-0110, ADR-0111, and
ADR-0112.** Steps 2 and 3 are planned here and proceed on the defaults
below. The choices marked "owner's call" stay open to change until
release.

The 0.16 survey of missing language features listed, as item 6:

> Only list, dict and set, with no deque, heap, ordered map or set
> algebra. Strings have no integer indexing or byte literals, and only
> UTF-8. There is no big integer, decimal or complex type, and `math`
> lacks inverse trig.

This checkpoint splits that into three steps, each its own pull
request, and settles the surface of each.

## Starting Point

- A list held in a local binding is fast: `append` and `pop` on a
  `list[int64]` cost about 16 instructions each.
- A list held in a class field is not. `self.items.append(value)` costs
  about 340 instructions and `self.items.pop()` about 650, because the
  call goes through the generic collection routine with boxed values.
- A generic function over `items: mut list[T]` that compares elements
  costs far more again. A binary heap written that way ran 400 times
  slower than Rust's `BinaryHeap` and 15 times slower than Python's
  `heapq`. The same code written for `int64` alone was 25 times slower
  than Rust, most of it in `swap`, which is a runtime call.
- A ring-buffer deque written as an Aura class over a list ran about
  9 times slower than Python's `deque`.
- Operator traits exist for `+ - * / // %` and ordering. Only `Add` and
  `Ord` are in the prelude. `| & ^ << >> **` have no trait.
- Builtin methods on `str` and `set` are table driven and shared by the
  interpreter and compiled code.

So features that live in the runtime (text, sets, `math`) can be added
at full speed now, and containers written in Aura cannot until the two
costs above are removed.

## Step 1: text, sets, and `math` (this pull request)

| Feature | Decision | Record |
| --- | --- | --- |
| Inverse trig and the rest of `math` | 27 functions and `tau`. NaN passes through, a domain error is `AU4001`, overflow is `AU4002`. Integer functions are exact on `int64`. | ADR-0110 |
| Byte literals | `b"..."` is a `list[uint8]`. ASCII and escapes only. | ADR-0111 |
| String integer indexing | `text[i]` and `text.char_at(i)` return a one-character `str` by scalar position. Constant time for ASCII. Read only. | ADR-0111 |
| Encodings beyond UTF-8 | `bytes.encode` and `bytes.decode` with eleven named encodings and a typed error. | ADR-0111 |
| Set algebra | Eight methods and the operators `\| & - ^` on `set[T]`. | ADR-0112 |

Owner's calls in step 1:

- **`text[i]` counts scalars and may scan.** The alternative is to keep
  refusing it. The default follows `len` and slicing, and the manual and
  the book say where the cost is.
- **No subset comparison operators.** `a <= b` is left out in favour of
  `is_subset`.

## Step 2: `numbers`

A source module `numbers` with three types.

- `BigInt`: an integer of any size. Literals convert with
  `BigInt.from_int(value)` and `BigInt.parse(text)`. The arithmetic
  operators, comparison, `pow`, `gcd`, `divmod`, `to_int() -> int64 |
  None`, and `str()` work. Values that fit one machine word stay in it,
  so small numbers do not allocate. Larger values use limbs in the
  runtime.
- `Decimal`: an exact base-10 number, a `BigInt` coefficient and an
  exponent. `+ - *` are exact. Division and `sqrt` take an explicit
  number of digits and a rounding mode, since they cannot be exact.
  Values are kept normalized so that `==` is numeric equality.
- `Complex`: a Copy class of two `float64` with the arithmetic
  operators, `abs`, `arg`, `conj`, `exp`, `sqrt`, and `polar`.

The module supplies the arithmetic operator traits it needs.
Bitwise operations, shifts and powers have named methods.
[ADR-0113](decisions/0113-numbers-module.md) records the representation and precision rules.

Owner's calls in step 2:

- **Names.** `BigInt`, `Decimal`, `Complex` in `numbers`, not
  lower-case builtin types. They are library types, as `Duration` is.
- **No implicit mixing.** `BigInt + int64` is an error; convert first.
  This is the rule every other numeric type follows.
- **Decimal has no global context.** Precision is an argument where a
  result cannot be exact. Python's thread-local context has no place to
  live in Aura.

## Step 3: `collections` (planned)

A source module `collections` with four types.

- `Deque[T]`: `push_back`, `push_front`, `pop_back`, `pop_front`,
  `front`, `back`, indexing, `len`, iteration, `clear`.
- `Heap[T]` for `T: Ord`: `push`, `pop`, `peek`, `len`, `from_list`,
  `into_sorted`. A min-heap. `Heap.max()` builds a max-heap.
- `SortedMap[K, V]` for `K: Ord`: `set`, `get`, `remove`, `contains`,
  `first`, `last`, `floor`, `ceiling`, `range(low, high)`, iteration in
  key order.
- `SortedSet[T]` for `T: Ord`: the same over keys alone.

The survey said "ordered map". `dict` already keeps insertion order,
so the missing type is the one ordered by key, and it is named for
that.

Before the types are written in Aura, the step removes the two costs
measured above: list operations on a field of `self`, and comparisons
and swaps in generic code over a list. Those fixes help every program
with a list in a class, not only this module. If they cannot reach the
bar, the fallback is to implement the four types in the runtime, as
`set` is, and the checkpoint is revised before that work starts.

Owner's calls in step 3:

- **A min-heap by default**, as in Python. Rust's is a max-heap.
- **`SortedMap` is a B-tree**, not a skip list or a red-black tree. It
  is the layout with the fewest allocations per element.

## The bar

Each step ships with benchmarks against Rust and Python. The target is
John's standing one: faster than Python and within about three times
of Rust. A row that misses is reported as a miss with the measured
ratio, not left out.

## Editor, verifier, and documents

- Editor: completion and hover come from the compiler's tables, so
  new members and functions appear without editor changes. The
  TextMate grammar gains the byte-string rule.
- `aura verify`: `text[i]` and the set methods are calls the verifier
  does not model, like the other `str` and `set` members. A contract
  that mentions them is checked at run time.
- Manual: Math, Bytes, Collections, Types, Expressions, Lexical
  Structure, Grammar, Current Limits, and the API index.
- Book: sections 3.2, 8.2, 8.3, and the operator and Python appendices.
