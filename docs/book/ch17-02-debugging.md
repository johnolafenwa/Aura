# Debugging in VS Code

When a program misbehaves, the first question is "where?" and the second is
"what were the values?". Aura answers the first one for free in most cases,
and a debug build plus a debugger answers the second. We will start with what
you already have, then build `scores` from the previous section for a
debugger and step through it, first in plain lldb so you can see exactly what
is happening, and then in VS Code, where the same thing is one key press.

## Start with the diagnostic

Look again at what the `scores` binary printed when it divided by zero:

```console
$ ./scores
week 1: 81
error[AU4004]: division by zero
 --> src/main.au:5:12
  |
5 |     return total // values.len()
  |            ^
  = note: Aura call chain (innermost first): average at 1:1 -> summarize at 7:1 -> main at 10:1
```

That is already most of a debugging session. The stable code `AU4004` names
the kind of trap. The caret shows the exact expression. The call chain shows
how we got there: `main` called `summarize`, which called `average`. In a
program with tasks, a trap in a child task adds the chain of tasks that led
to it. Every Aura binary carries the metadata to print this, so you get it
from a program that failed in production as you do on your laptop.

What the diagnostic does not tell you is *why* `values.len()` was zero, or
which call to `summarize` it was. Here it is obvious from reading `main`, but
in a real program the empty list might have come from a file, a network
response, or a filter three functions away. The cheapest next step is often
to print. `print` writes to standard output; if your program's output is
being read by another tool, the `log` module writes one JSON record per call
to standard error instead, which keeps diagnostic noise out of the data:

```aura
import log

def main():
    log.info("checking", {"name": "web", "attempt": "1"})
    print("done")
```

```console
$ aura run logdemo.au
{"fields":{"attempt":"1","name":"web"},"kind":"log","level":"info","message":"checking"}
done
$ aura run logdemo.au 2>/dev/null
done
```

When printing turns into printing everything, it is time for a debugger.

## A debug build

A normal `aura build` produces an optimized binary without the information a
debugger needs to map machine code back to your source. Ask for a debug build
with `--debug`:

```console
$ aura build --debug -o scores-debug src/main.au
aura: built `scores-debug` with direct backend (debug)
```

A debug build is different from a normal one in three ways. The code is not
optimized, and every Aura statement gets its own line-table entry, so a
breakpoint on a line stops on that line and stepping moves one statement at a
time. Every function appears by name in the call stack, and every local
variable that fits in one machine word is described along with its Aura type.
And nothing is stripped. On macOS the debug information is written to a
`.dSYM` bundle next to the binary.

The cost is speed and size. A debug build runs slower, and it is larger: with
a release compiler on a Linux x86-64 container, `scores-debug` is 16,209,976
bytes against 1,296,768 for the normal build. Use debug builds for debugging,
not for shipping.

## Stepping through with lldb

Aura's debug builds work with LLVM's debugger, lldb. Values look much better
with the formatter that ships with the VS Code extension, a Python script at
*debug/aura_lldb.py* inside the extension (and at
*tools/vscode-aura/debug/aura_lldb.py* in the Aura repository). Let's start
lldb, load the formatter, and stop on line 5, the division:

```console
$ lldb ./scores-debug
(lldb) target create "./scores-debug"
Current executable set to '/home/you/scores/scores-debug' (x86_64).
(lldb) command script import /home/you/aura/tools/vscode-aura/debug/aura_lldb.py
(lldb) breakpoint set --file main.au --line 5
Breakpoint 1: where = scores-debug`average + 750 at main.au:5:5, address = 0x000000000012a6ae
(lldb) run
Process 646 launched: '/home/you/scores/scores-debug' (x86_64)
Process 646 stopped
* thread #3, name = 'aura-worker-0', stop reason = breakpoint 1.1
    frame #0: 0x000055555567e6ae scores-debug`average at main.au:5:5
   2   	    mut total: int64 = 0
   3   	    for value in values:
   4   	        total += value
-> 5   	    return total // values.len()
    	    ^
   6   	
   7   	def summarize(name: str, values: list[int64]):
   8   	    print(f"{name}: {average(values)}")
```

The process stopped in `average`, on our line, and lldb shows the source
around it. Notice the thread name, `aura-worker-0`. `main` does not run on
the process's original thread; it runs as the root task on one of the
scheduler's worker threads, like every other Aura task. A program with a task
group will show you several `aura-worker-N` threads.

The call stack comes from `bt`. The full stack continues down into the
runtime's scheduler, so ask for the top three frames:

```console
(lldb) bt 3
* thread #3, name = 'aura-worker-0', stop reason = breakpoint 1.1
  * frame #0: 0x000055555567e6ae scores-debug`average at main.au:5:5
    frame #1: 0x000055555567eee8 scores-debug`summarize at main.au:8:5
    frame #2: 0x000055555567ecd1 scores-debug`main at main.au:11:5
```

Frame 2 says `main` is on line 11, the first call to `summarize`. This is the
call that works, so let it continue to the next stop and look at the local
variables there with `frame variable`:

```console
(lldb) continue
week 1: 81
Process 646 resuming
Process 646 stopped
* thread #3, name = 'aura-worker-0', stop reason = breakpoint 1.1
    frame #0: 0x000055555567e6ae scores-debug`average at main.au:5:5
   2   	    mut total: int64 = 0
   3   	    for value in values:
   4   	        total += value
-> 5   	    return total // values.len()
    	    ^
   6   	
   7   	def summarize(name: str, values: list[int64]):
   8   	    print(f"{name}: {average(values)}")
(lldb) frame variable
(list[int64]) values = []
(int64) value = 0
(int64) total = 0
```

There is the empty list. Each variable is shown with its Aura type, and the
list is shown the way `print` would write it. Move up one frame to see who
passed it in:

```console
(lldb) up
frame #1: 0x000055555567eee8 scores-debug`summarize at main.au:8:5
   5   	    return total // values.len()
   6   	
   7   	def summarize(name: str, values: list[int64]):
-> 8   	    print(f"{name}: {average(values)}")
    	    ^
   9   	
   10  	def main():
   11  	    summarize("week 1", [70, 85, 90])
(lldb) frame variable
(str) name = "week 2"
(list[int64]) values = []
(lldb) kill
Process 646 exited with status = 9 (0x00000009) killed
```

It was the `"week 2"` call. On the first stop, by the way, the same
`frame variable` showed `values = [70, 85, 90]`, `value = 90`, and
`total = 245`, the state right after the loop.

Without the formatter script, lldb still finds the variables and their types,
but it can only show the raw representation of runtime values. The list from
the first stop then reads `(list[int64]) values = (address = 140737018598368)`,
which tells you nothing. Numbers and `bool` look the same either way.

## Debugging in VS Code

The Aura extension for VS Code wraps all of this. Install it as described in
[Appendix D](/book/appendix-04-tools), and make sure you have `lldb-dap`, LLVM's
debug adapter. On macOS it comes with the Xcode command line tools
(`xcode-select --install`). On Linux, your distribution's `lldb` package
provides `lldb-dap` or a versioned `lldb-dap-<version>`.

Then open *src/main.au* and do any one of these:

- press F5,
- click the **Debug** lens that appears above `def main`,
- run **Aura: Debug File** from the Command Palette.

The extension runs `aura build --debug` on the file, puts the binary in its
own storage directory rather than your workspace, and starts `lldb-dap` on
it. From there it is an ordinary VS Code debugging session. Click in the
gutter next to line 5 to set a breakpoint, and the program stops there with
the Variables panel showing `values`, `value`, and `total` as Aura values, the
Call Stack panel showing `average`, `summarize`, and `main`, and the usual
step over, step into, and continue buttons working one Aura statement at a
time. The formatter is loaded for you.

To pass arguments, set a working directory, or add environment variables, add
a configuration of type `aura` to *.vscode/launch.json*. For `checkup`, which
reads its config path from the command line and looks for files relative to
its working directory, that might be:

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "aura",
      "request": "launch",
      "name": "Debug checkup",
      "program": "${workspaceFolder}/src/main.au",
      "args": ["checks.json"],
      "cwd": "${workspaceFolder}"
    }
  ]
}
```

`program` is the `.au` file to build, not a binary. `args`, `cwd`, `env`, and
`stopOnEntry` mean what they mean in any VS Code debugger; without `cwd`, the
program starts in the directory that holds `program`. If the extension cannot
find `lldb-dap`, set the `aura.lldbDapPath` setting to its full path. Without
that setting it looks for Xcode's copy through `xcrun` on macOS, and then on
`PATH` for `lldb-dap`, `lldb-dap-<version>`, or the older name
`lldb-vscode`.

## What the debugger cannot show yet

Debug support is new in Aura 0.11, and some values are not described yet.
Unions (such as `str | None`), callables, and instances of a class with a
field that is itself more than one machine word do not show their contents.
lldb's expression evaluator speaks C, not Aura. Arithmetic on simple numbers
works, so at the first stop `print total * 2` printed `(int64) 490`, but you
cannot call Aura methods or write Aura expressions at the prompt.

One effect will surprise you if you are used to garbage-collected languages.
In compiled code, a value can be released as soon as its last use has passed,
which may be a few lines before the end of its scope. If you stop on a line
after a binding's last use, the debugger may show it as `<unset>` or null.
That is not a bug in your program; the compiler knows nothing will read the
value again, so it gave the memory back early. Set the breakpoint earlier, or
use the value later, if you need to see it.

Debugging tells you why a program is wrong. The next section is about the
other thing people want to know about a working program: whether it is fast
enough.
