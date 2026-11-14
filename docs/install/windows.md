# Install Aura On Windows

Aura runs natively on x86-64 Windows 11. The native Windows archive ships
from the release after 0.15. On 0.15, or on a machine without the Microsoft
C++ build tools, use [Windows with WSL](/install/windows-wsl).

## 1. Install The Microsoft Linker

`aura run` and `aura build` compile a program to native code and link it
with the Microsoft linker, `link.exe`. Install the
[Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022)
and select the **Desktop development with C++** workload. A full Visual
Studio install with that workload works too.

`aura run --backend mir`, `aura check`, `aura fmt`, and the language server
need no linker.

## 2. Install Aura

In PowerShell:

```powershell
irm https://johnolafenwa.github.io/Aura/install.ps1 | iex
```

The installer downloads the `x86_64-pc-windows-msvc` archive and
`SHA256SUMS`. It rejects the archive if the checksum does not match, and
otherwise installs under `%LOCALAPPDATA%\Programs\Aura`.

## 3. Add Aura To PATH

Add the `bin` directory to your user `PATH`. You only need to do this once:

```powershell
$bin = "$env:LOCALAPPDATA\Programs\Aura\bin"
[Environment]::SetEnvironmentVariable('Path', "$bin;" + [Environment]::GetEnvironmentVariable('Path', 'User'), 'User')
$env:PATH = "$bin;$env:PATH"
```

New terminals read the user `PATH`. The last command updates the current
terminal right away.

## 4. Verify The Installation

```powershell
aura --version
```

## 5. Run A Program

```powershell
aura run "$env:LOCALAPPDATA\Programs\Aura\share\aura\examples\basic_addition.au"
```

The program prints `16`. To write a standalone executable:

```powershell
aura build -o basic_addition.exe "$env:LOCALAPPDATA\Programs\Aura\share\aura\examples\basic_addition.au"
.\basic_addition.exe
```

## What Gets Installed

```text
%LOCALAPPDATA%\Programs\Aura\
├── bin\aura.exe
├── lib\aura\
│   ├── aura_runtime.lib
│   ├── native-link-args.json
│   └── windows.*.lib
└── share\aura\
    ├── examples\
    ├── README.md
    └── LICENSE
```

To install under a different directory, set `AURA_INSTALL_PREFIX` before
running the installer.

Aura keeps built programs, proofs and git dependencies under
`%USERPROFILE%\.cache\aura`. Set `AURA_CACHE_DIR` to keep built programs
and proofs elsewhere.

## Install With Chocolatey Instead

`choco install aura --pre` installs the same files as a Chocolatey
package. See
[Install with a package manager](/install/package-managers#chocolatey).

## Differences From Linux And macOS

- A program's executable has the `.exe` extension, and `aura build --debug`
  writes a `.pdb` program database beside it.
- Packages that name native libraries under `[ffi]` link import libraries
  (`name.lib`) and load the matching DLLs from `PATH`.
