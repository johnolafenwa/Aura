# Outline of the Aura book

Working outline for *The Aura Programming Language*. Not published. File names
follow `chNN-MM-slug.md`; `-00-` is the chapter landing page.

The book replaced the old Learn track (`docs/learn/`) and `tutorials/` on
2026-11-01; both are in git history if you need their examples.

## Front matter

- `index.md`: title page, what Aura is, who the book is for, how to read it,
  how examples are checked, where the source lives.
- `foreword.md`: why Aura exists (Python's readability with a compiler that
  checks ownership and failure, native binaries, structured concurrency).

## 1. Getting started

- 1.1 Installation (`ch01-01-installation.md`)
- 1.2 Hello, world (`ch01-02-hello-world.md`): a script, `aura run`, `main`
  and exit codes, `aura build` and the native binary.
- 1.3 Hello, packages (`ch01-03-hello-packages.md`): `aura new`, `Aura.toml`,
  `src/main.au`, `aura check`, `aura test`, editor setup.

## 2. Programming a word counter (`ch02-00-word-counter.md`)

One-page project: read text from standard input with `io.read_line`, split
into words, normalize case, count in a `dict`, sort, print the top N. Meets
bindings, `mut`, `match` on `Result`, loops, collections, f-strings in passing
and promises to explain each later.

## 3. Common programming concepts

- 3.1 Bindings and mutability
- 3.2 Data types (integers, floats, bool, str, None, tuples, `as` and
  conversions, why nothing converts implicitly)
- 3.3 Functions (parameters, defaults, named arguments, return types,
  lambdas briefly)
- 3.4 Comments and docstrings
- 3.5 Control flow (`if`/`elif`, `while`, `for`, ranges, `break`/`continue`,
  multiline expressions)

## 4. Understanding ownership

- 4.1 What is ownership (copy values vs owned values, moves, scope and
  cleanup, no garbage collector)
- 4.2 Shared and mutable access (bare, `mut`, `own` parameters; receivers;
  `for value in mut`)
- 4.3 Copies and slices (`.copy()`, owned list and str slices, cost)

## 5. Using classes to structure data

- 5.1 Defining and building classes
- 5.2 An example program using classes (grow a small program from loose
  bindings to a class)
- 5.3 Methods (`self`, `mut self`, `own self`, properties, `__str__`, no
  inheritance and why)

## 6. Enums and pattern matching

- 6.1 Defining an enum (payloads, recursive enums)
- 6.2 `match` (exhaustiveness, literal cases, guards, bindings)
- 6.3 `T | None` and concise matching

## 7. Managing growing programs with modules and packages

- 7.1 Modules and imports
- 7.2 Visibility and private names
- 7.3 Packages, dependencies, and workspaces

## 8. Common collections

- 8.1 Lists
- 8.2 Strings (UTF-8, slicing, split/replace, building strings)
- 8.3 Dictionaries and sets
- 8.4 Comprehensions and builtins (`sum`, `min`, `sorted`, `reversed`, ...)
- 8.5 Numeric arrays (`Array[T]`)
- 8.6 Views and spans (`view` bindings, `-> view T from`, `Span[T]`)
- 8.7 Lane values (the `simd` module, a worked and timed lane loop)

## 9. Error handling

- 9.1 Traps: failures that stop a task (assertions, index errors)
- 9.2 Recoverable errors with `Result` (`try`, domain error enums,
  `Lookup[T]`)
- 9.3 To trap or not to trap

## 10. Generics, traits, and conversions

- 10.1 Generic functions and classes
- 10.2 Traits
- 10.3 Converting between types
- 10.4 Iterators and generators
- 10.5 Trait objects (`dyn Trait`, conversion, `match` type arms, when to use a generic, a union, or a trait object)

## 11. Writing automated tests

- 11.1 How to write tests
- 11.2 Running tests and reading failures
- 11.3 Organizing tests (parameterized cases, CI output)
- 11.4 Contracts and proofs (`requires`, `ensures`, `invariant`, `aura verify`)

## 12. An I/O project: building a log analyzer

- 12.1 Reading the arguments and the file
- 12.2 Parsing lines into records
- 12.3 Aggregating and reporting
- 12.4 Refactoring with tests and better errors

## 13. Closures and decorators

- 13.1 Closures (captures, single-use closures, function types)
- 13.2 Decorators (and the typed JSON codecs they derive)

## 14. Resources and cleanup

- 14.1 `with` and deterministic cleanup
- 14.2 Writing your own resource manager (traps and cleanup order)

## 15. Structured concurrency

- 15.1 Tasks and task groups
- 15.2 Passing work with queues
- 15.3 Cancellation, timeouts, and shutdown
- 15.4 A worker pool

## 16. Talking to the world

- 16.1 Files and the filesystem
- 16.2 Running processes and supervisors
- 16.3 Networking and HTTP
- 16.4 JSON
- 16.5 HTTP clients (the `http` module, posting, server-sent events,
  `control.retry`)
- 16.6 Other protocols (UDP, WebSockets, Unix sockets, TLS)

## 17. Building, debugging, and shipping

- 17.1 `aura run` vs `aura build`, and what a native binary contains
- 17.2 Debugging in VS Code
- 17.3 Performance (what is fast, what costs, how to measure)
- 17.4 Building for other machines (`--target`, target packs, static musl,
  arm64, `--target-cpu`)

## 18. Calling C

- 18.1 FFI (`ch18-00-calling-c.md` single page is fine)
- 18.2 Without an operating system: `export "C"`, freestanding builds, the trap hook, the `mem` module, booting under QEMU

## 19. Final project: a supervised job runner

- 19.1 A single-task runner
- 19.2 Running jobs in parallel with a worker pool
- 19.3 Supervision, restarts, and graceful shutdown

## 20. A tour of the standard library

One small tool, `courier`, grows across the sections.

- 20.1 Bytes and codecs (gzip, SHA-256, HMAC, hex, base64)
- 20.2 Time and dates
- 20.3 Regular expressions
- 20.4 Randomness (seeded and secure)
- 20.5 Logging, tracing, and metrics

## 21. Tensors

The first of six chapters on machine learning. They assume no experience
with it and use the `tensor` module on the CPU device.

- 21.1 Tensors, shapes, and devices
- 21.2 Operations and broadcasting
- 21.3 Gradients (`requires_grad`, `backward()`, gradient descent by hand)

## 22. Learning from data

- 22.1 Linear regression (`Linear`, a loss, `SGD`, the `mut` rule for models)
- 22.2 Classification (`cross_entropy`, `softmax`, accuracy)
- 22.3 The training loop (batches, held-out data, overfitting, weight decay,
  `AdamW`)

## 23. Neural networks

- 23.1 Hidden layers (why one layer fails, activations)
- 23.2 Models and modules (`impl Module`, `Sequential`, saving and loading)
- 23.3 Convolutional networks (`Conv2d`, pooling, `BatchNorm2d`, training and
  evaluation modes)

## 24. A transformer from scratch

A project chapter: a character model trained on Shakespeare.

- 24.1 Text as numbers (a vocabulary, batches of windows, a table baseline)
- 24.2 Attention (queries, keys, values, the causal mask, heads)
- 24.3 The model and its training (embeddings, blocks, residuals, layer norm)
- 24.4 Making it write (the generation loop, temperature)

## 25. Running GPT-2

A project chapter: GPT-2 inference from the published weights.

- 25.1 The weights (safetensors, reading a model's design from its names)
- 25.2 The tokenizer (byte pair encoding from `vocab.json` and `merges.txt`)
- 25.3 The model (field names as the loading contract, weight tying)
- 25.4 Generating text (top-k sampling, speed, the key-value cache)

## 26. Devices and speed

- 26.1 Running on a GPU (Metal and CUDA requirements, when the work runs,
  small work on the CPU, measured times)
- 26.2 Coming from PyTorch (differences and why, a phrase book, what is not
  there yet)

## Appendices

- A: Keywords
- B: Operators and symbols
- C: Aura for Python developers
- D: Useful tools (formatter, language server, VS Code)
- E: Diagnostics you will meet often
- F: Following the book on Windows
