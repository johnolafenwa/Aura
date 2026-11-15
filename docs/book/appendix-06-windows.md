# Following the book on Windows

Aura runs natively on x86-64 Windows 11, and almost everything in this book
is about Aura, which is the same there as anywhere. What differs is the
ground the examples stand on. The console listings were recorded in a macOS
or Linux shell, and a few chapters call Unix programs such as `/bin/sh`.
This appendix is for the reader in PowerShell who wants to know what to
type.

You have two routes, and you can mix them.

The first is WSL 2, the Windows Subsystem for Linux.
[Section 1.1](/book/ch01-01-installation) shows how to install Ubuntu under
it. Inside the Ubuntu terminal you are a Linux user: every listing in the
book works as printed, and you can stop reading here.

The second is native Windows, which the rest of this appendix covers. It
produces real Windows programs and needs a little translation. We will be
plain about one thing first. The project tests Aura on Windows in
continuous integration, running the compiler, the runtime, and the command's
own test suites there, but the book's listings were run on Linux and macOS.
Where a chapter leans on Unix tools, we say so below and suggest WSL for
that chapter.

## What you need installed

Install Aura with the PowerShell command from Section 1.1 and add its `bin`
directory to your `PATH`. That is enough for most of the book:

```console
> aura --version
aura 0.16.0-preview (91218b26b464)
> aura run hello.au
Hello, world!
```

`aura run`, `aura check`, `aura test`, and `aura fmt` need nothing else.
`aura build` and `aura run --backend direct` produce native code and link
it with Microsoft's linker, so they need the Visual Studio Build Tools with
the **Desktop development with C++** workload. Without them, the build
stops with a message that says what to install:

```console
> aura build -o hello.exe hello.au
error[AU2999]: cannot find the Microsoft linker `link.exe`: install the Visual Studio Build Tools with the "Desktop development with C++" workload
 --> hello.au
```

You will want the build tools by the end of [Chapter 1](/book/ch01-02-hello-world),
where we first build a binary.

## Reading the console listings

The book's listings start each command with `$`, the prompt of a Unix
shell. Yours shows `PS C:\...>`. The `aura` commands themselves are typed
the same way. The differences are in what surrounds them:

| In the book | In PowerShell |
| --- | --- |
| `aura build -o greeter src/main.au` | `aura build -o greeter.exe src/main.au` |
| `./greeter Ada` | `.\greeter.exe Ada` |
| `echo $?` | `echo $LASTEXITCODE` |
| `mkdir -p a/b` | `mkdir a\b` |
| `cat notes.txt`, `ls`, `cp`, `rm`, `cd` | the same words work as PowerShell aliases |
| `export NAME=value` | `$env:NAME = "value"` |
| `NAME=value aura run src/main.au` | `$env:NAME = "value"; aura run src/main.au` |
| `aura run src/main.au 2> errors.log` | the same |
| `time ./program` | `Measure-Command { .\program.exe }` |
| `curl -s -i http://127.0.0.1:8080/health` | `curl.exe -s -i http://127.0.0.1:8080/health` |
| `file program`, `ldd program` | no direct equivalent |

Three of these deserve a sentence. Windows runs a program by its `.exe`
extension, so add the extension wherever a listing names an output with
`-o`. `$?` exists in PowerShell but holds `True` or
`False`. The number that `main` returned is in `$LASTEXITCODE`. And an
environment variable set with `$env:` stays set for the rest of the
session, where the Unix form `NAME=value command` applies to one command.
Remove it with `Remove-Item Env:NAME` when a listing's next command is
meant to run without it.

Write `curl.exe` with its extension. In older versions of PowerShell, plain
`curl` is an alias for a different command with different options.

## Paths and source files

Aura accepts both kinds of slash in a path, on the command line and inside
a program. `aura check src/main.au` and `aura check src\main.au` are the
same command, and `fs.exists("src/main.au")` finds the same file as
`fs.exists("src\\main.au")`. Keep writing forward slashes, as the book does, and
your programs stay portable.

Diagnostics print the full Windows path, so where the book shows

```text
error[AU2002]: binding `x` has annotated type `int64`, but value has type `str`
 --> src/main.au:2:5
```

you will see the same message with a location such as
`C:\Users\ada\projects\hello\src\main.au:2:5`.

The compiler reads source files with Windows line endings without
complaint. The formatter writes Unix line endings, and `aura fmt --check`
reports a file with Windows line endings as needing formatting. If Git on
your machine converts line endings on checkout, tell it to leave Aura files
alone by adding one line to a *.gitattributes* file in your package:

```text
*.au text eol=lf
```

Aura keeps built programs and other caches under
`%USERPROFILE%\.cache\aura`, where the book says `~/.cache/aura`.

## Standard input

Chapters [2](/book/ch02-00-word-counter) and
[12](/book/ch12-00-log-analyzer) feed files to programs with `<`, which
PowerShell does not support. Pipe the file in instead:

```console
> Get-Content poem.txt | aura run src/main.au
```

`echo "to be or not to be" | aura run src/main.au` works as printed. If a
file with characters outside ASCII comes through garbled, run the command
through the classic command prompt, which does support `<` and passes the
bytes through untouched:

```console
> cmd /c "aura run src/main.au < poem.txt"
```

## Chapter by chapter

Most chapters need nothing beyond the table above. These are the ones that
do.

**Chapter 16, section 2, and Chapter 19** run other programs, and the ones
they run are Unix programs: `/bin/echo`, `/bin/sleep`, `/usr/bin/sort`, and
shell scripts through `/bin/sh -c`. The `process` module itself works on
Windows, with no shell in between, exactly as those chapters describe. A
command is still a list of strings, so the Windows spelling of
`["/bin/echo", "hello"]` is `["cmd", "/c", "echo", "hello"]`:

```aura
import process

def main():
    print(process.run(["cmd", "/c", "echo", "hello"]))
```

```console
> aura run src/main.au
Result.Ok(<process-completed ExitStatus.Exited(0)>)
```

The longer examples depend on shell features (`kill -9 $$`, `$(pwd)`, and
the job scripts of the final project) that have no one-line translation.
Read those two chapters under WSL. Process groups, which `group=true`
requests, are also not available on Windows.

**Chapter 16, section 6** uses the path `/tmp/courier.sock` for a Unix
domain socket. Unix sockets work on Windows. Choose a path that exists
there, such as a file under the directory that `sys.env("TEMP")` names. The
`openssl` command in the TLS example is included with Git for Windows, in
its Git Bash shell.

**Chapter 17** inspects binaries with `file` and `ldd`, which Windows does
not have. A debug build with `aura build --debug` writes a `.pdb` file
beside the executable in place of the debug information described there.
The lldb session in section 2 was recorded on Linux, and we have not
repeated it on Windows. Section 4's `aura build --target` runs only on a
Linux host. A Windows machine builds Windows programs, and WSL builds Linux
ones, including the static kind.

**Chapter 18** compiles small C files with `cc` and links them into Aura
packages. On Windows a package that names a native library links the
library's import file (`name.lib`) and loads the matching DLL from `PATH`.
The chapter's build commands are for a Unix toolchain, and the freestanding
kernel at its end is built and booted with Linux tools. Use WSL for it.

Everything else, which is to say the language itself, the collections,
traits, tests, contracts, tasks and queues, files, HTTP, JSON, and the
library tour in Chapter 20, behaves on Windows as the book describes.
