# Fresh-Eyes Corpus Lane B: Programs 09-16

Date: 2026-07-31

## Method

These eight programs were written as practical day-2/day-3 tasks using only the
maintained root README, tutorials, and manual chapters. Compiler tests,
fixtures, and existing examples were deliberately not inspected or searched.

The principal reference pages used were:

- `tutorials/03-functions.md`
- `tutorials/04-control-flow.md`
- `tutorials/07-strings-and-numbers.md`
- `tutorials/09-enums-and-match.md`
- `tutorials/10-results-and-options.md`
- `tutorials/12-error-propagation.md`
- `tutorials/19-io-and-networking.md`
- `tutorials/21-json.md`
- `tutorials/22-bytes.md`
- `docs/manual/collections.md`
- `docs/manual/filesystem.md`
- `docs/manual/process.md`

The recorded run used `target/debug/aura` (`aura 0.1.0`) from repository HEAD
`86ff95a9ffcf31f4d8183a72e67f3619a2d255dc`. Every final source also passed
`target/debug/aura fmt --check`.

For each program, both of these commands were run with stdout and stderr
captured separately:

```text
target/debug/aura run --backend mir work/fresh-eyes-corpus/<program>.au
target/debug/aura run --backend direct work/fresh-eyes-corpus/<program>.au
```

## Final Results

All 16 final executions exited 0. MIR and direct produced byte-identical
program stdout for every program. MIR stderr was empty. Direct stderr was the
same CLI status line on every run: `aura: building native program...\n`.
This is runner/build status rather than program output, so application behavior
had full backend parity; raw stderr itself was intentionally not identical.

| No. | Program | Practical task | MIR | Direct | Exact program stdout | Parity |
| --- | --- | --- | --- | --- | --- | --- |
| 09 | `09-string-parsing-formatting.au` | Split, validate, parse, normalize, and format one record | exit 0; stderr `""` | exit 0; stderr `"aura: building native program...\n"` | `"name=AURORA; count=42; ratio=3.5\n"` | exit/stdout yes |
| 10 | `10-text-file-roundtrip.au` | Text write, append, read, normalize, remove | exit 0; stderr `""` | exit 0; same direct status line | `"alpha,beta\nfalse\n"` | exit/stdout yes |
| 11 | `11-byte-file-roundtrip.au` | Raw-byte write/read, hex verification, remove | exit 0; stderr `""` | exit 0; same direct status line | `"0001027f80feff\nfalse\n"` | exit/stdout yes |
| 12 | `12-directory-listing.au` | Create two files, list `/private/tmp`, find exact names, remove | exit 0; stderr `""` | exit 0; same direct status line | `"first=true; second=true\n"` | exit/stdout yes |
| 13 | `13-json-config.au` | Parse a JSON config, validate an integer field, dump deterministically | exit 0; stderr `""` | exit 0; same direct status line | `"workers=3\n{\"mode\":\"batch\",\"ready\":true,\"workers\":3}\n"` | exit/stdout yes |
| 14 | `14-process-result.au` | Execute argv without a shell, check status, inspect captured output | exit 0; stderr `""` | exit 0; same direct status line | `"success=true; output=aurora-process\n"` | exit/stdout yes |
| 15 | `15-result-try-pipeline.au` | Parse/validate/divide through a multi-stage `Result`/`try` pipeline | exit 0; stderr `""` | exit 0; same direct status line | `"ok=31\nerror=value must be positive\n"` | exit/stdout yes |
| 16 | `16-data-cleaning.au` | Filter, trim, lowercase, replace, collect, and report over text rows | exit 0; stderr `""` | exit 0; same direct status line | `"count=3; labels=beta-team,alpha-team,gamma\n"` | exit/stdout yes |

Raw captures are at
`/private/tmp/aurora-fresh-eyes-lane-b-<program>-<mir|direct>.<stdout|stderr>`.
The MIR/direct stdout SHA-256 values are identical per program:

| No. | SHA-256 |
| --- | --- |
| 09 | `659326243940ab2b1a9f889b0dcf86743606645c696813cdffeb7fa69ef02cf6` |
| 10 | `10905e715c1432a8ab7d4229c0f15208f4728543aade3098ed692f41eae6d201` |
| 11 | `79b9049bfee6f9d2802addcc85401cb547940cde8c885f9c69dece0a35fc098a` |
| 12 | `83e9206570ace288172ec656ab0ccaf6c7cfb9a603bf23d835c40ab9c4bcfca3` |
| 13 | `49ab8d63255c71d09915513835e9f3e117673df1c0afeea4317905129c83ed57` |
| 14 | `1b3e3dcd484fa681e85d5c5812abb896a57fffbd506f6b1f5583307b3bf7f32f` |
| 15 | `048a5cc96db254f4e438f930ebfbab07b235d795dd9090039bf9340fb8246a6c` |
| 16 | `698ff551180df1412fdc0c63d3a46d9fff16ce45024d0166e9a79d0ae20ff6ae` |

The direct status-line stderr hash is
`898d38ec821c399d32c298b7e5945894ffe01957623d8b3cb3ad5a13852f97a5`;
the empty MIR stderr hash is
`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.

All four temporary application files were confirmed absent after the final
runs:

```text
/private/tmp/aurora-fresh-eyes-lane-b-text.txt
/private/tmp/aurora-fresh-eyes-lane-b-bytes.bin
/private/tmp/aurora-fresh-eyes-lane-b-directory-a.txt
/private/tmp/aurora-fresh-eyes-lane-b-directory-b.txt
```

## Fresh-User Findings

### F1: a bare enum match borrows a non-copy payload

Programs 09 and 13 initially used bare `match` and then moved a bound payload.
This was user error, not a compiler defect: the maintained match documentation
explains that bare matching retains the scrutinee and `match own` yields owned
payloads. The compiler diagnostics were precise and included usable fixes.

Program 09's exact first failure (both backends, exit 1, empty stdout) was:

```text
error[AU3002]: cannot move borrowed value `value`
 --> /Users/johnolafenwa/source2/Aurora/work/fresh-eyes-corpus/09-string-parsing-formatting.au:4:30
  |
4 |             return Result.Ok(value)
  |                              ^
  = related /Users/johnolafenwa/source2/Aurora/work/fresh-eyes-corpus/09-string-parsing-formatting.au:3:26: value is borrowed here
  = help: take `value` as `own String` when ownership is required, or call `.clone()` to consume an independent copy
  = fix: replace /Users/johnolafenwa/source2/Aurora/work/fresh-eyes-corpus/09-string-parsing-formatting.au:4:35-4:35 with `.clone()`
```

The source was corrected to `match own fields.get(index)`.

Program 13's exact first failure (both backends, exit 1, empty stdout) was:

```text
error[AU3002]: cannot move borrowed value `config`
 --> /Users/johnolafenwa/source2/Aurora/work/fresh-eyes-corpus/13-json-config.au:13:48
   |
13 |             print(json.dumps(json.Value.Object(config)))
   |                                                ^
  = related /Users/johnolafenwa/source2/Aurora/work/fresh-eyes-corpus/13-json-config.au:5:42: value is borrowed here
  = help: take `config` as `own Map[String, json.Value]` when ownership is required, or call `.clone()` to consume an independent copy
  = fix: replace /Users/johnolafenwa/source2/Aurora/work/fresh-eyes-corpus/13-json-config.au:13:54-13:54 with `.clone()`
```

The source was corrected to `match own json.parse(source)`.

### F2: strings cannot currently be sorted with `Vec.sort`

Program 16 initially attempted the ordinary Python-like cleanup step
`labels.sort()`. This is a current language/library limitation rather than a
compiler bug: `Vec.sort` requires an orderable element type, but `String` has
no built-in `Ord[String]` implementation. The final program preserves input
order.

The exact first failure (both backends, exit 1, empty stdout) was:

```text
error[AU2002]: `Vec.sort` cannot order Vec element type `String`
 --> /Users/johnolafenwa/source2/Aurora/work/fresh-eyes-corpus/16-data-cleaning.au:12:12
   |
12 |     labels.sort()
   |            ^
  = help: use an existing naturally ordered type, or implement `Ord[T].lt` returning `bool`
```

This is also mild documentation friction: the `Vec.sort` table says it sorts
an "orderable vector", but a Python newcomer is likely to assume String is a
naturally ordered built-in. The collections manual should enumerate the
built-in orderable types or explicitly call out that String ordering is not
provided.

#### F2 documentation closure

The discoverability gap is now closed without changing compiler behavior.
The normative Collections Manual and API index explicitly enumerate Aurora
0.2's built-in orderable types as all integer types, `float32`, `float64`, and
`Duration`, and explicitly state that `String` has no built-in `Ord[String]`.
The learning collection guide, current-language tutorial, and current-limits
page carry the same rule and practical migrations:

- preserve insertion order when ordering is unnecessary or already meaningful
- use `sort_by` with an orderable key/index, such as `String.len()`
- define a nominal application type with an `Ord` implementation over an
  explicit domain rank/key

Updated maintained pages:

- `docs/manual/collections.md`
- `docs/manual/api-index.md`
- `docs/learn/collections.md`
- `docs/manual/current-limits.md`
- `tutorials/14-current-language-surface.md`

Verification:

- `npm run check:reference` passed, including 123 verified Aurora Manual
  blocks, all 683 migrated manifest files, and the retired-syntax guard
- `npm run docs:build` passed; VitePress rendered the site successfully
- `git diff --check` passed for the Lane B programs, report, and documentation

## Verdict

No compiler or runtime correctness bug was found in this lane. All eight
practical programs work on both backends after two documented ownership fixes
and one adaptation around the documented typeclass requirement. The strongest
remaining product limitation exposed here is the absence of built-in String
ordering for routine data-cleaning scripts; its prior discoverability gap is
now closed in the maintained reference and teaching surfaces. The repeated
direct-run rebuild status line is a tooling observation worth checking during
the consolidated corpus review, but it did not change exit status or
application output.
