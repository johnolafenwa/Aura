# Install Aura On Windows With WSL

Aura does not publish a native Windows executable. On an x86-64 Windows 11
machine, install and run the Linux release inside Windows Subsystem for Linux
2 using Ubuntu 24.04. The CLI, compiler runtime, projects, and VS Code language
server all run inside WSL 2.

## 1. Install WSL 2 And Ubuntu

Open PowerShell as Administrator. Check the available distribution names:

```powershell
wsl --list --online
```

Install Ubuntu 24.04:

```powershell
wsl --install -d Ubuntu-24.04
```

Restart Windows if requested. Launch **Ubuntu 24.04 LTS** from the Start menu
and create the Linux username and password requested on first launch.

Microsoft's [WSL installation guide](https://learn.microsoft.com/windows/wsl/install)
documents recovery steps for older Windows builds and existing WSL setups.

## 2. Confirm WSL 2

In PowerShell:

```powershell
wsl --list --verbose
```

The Ubuntu row must show version `2`. If it shows version `1`, use the exact
distribution name displayed by the preceding command:

```powershell
wsl --set-version Ubuntu-24.04 2
```

All remaining shell commands in this guide run inside the Ubuntu terminal,
not PowerShell.

## 3. Prepare Ubuntu

```bash
sudo apt update
sudo apt install -y curl ca-certificates tar coreutils build-essential
```

The first four packages install and verify Aura. `build-essential` enables
direct native execution and `aura build`.

## 4. Install Aura Inside WSL

```bash
curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh
```

Add Aura to the Ubuntu login environment:

```bash
grep -qxF 'export PATH="$HOME/.local/bin:$PATH"' "$HOME/.profile" || \
  printf '%s\n' 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.profile"
export PATH="$HOME/.local/bin:$PATH"
```

Verify both the CLI and native toolchain:

```bash
aura --version
cc --version
```

## 5. Create A Project In The WSL Filesystem

Keep active Aura projects under the Linux home directory. This gives Linux
tools normal permissions and filesystem behavior:

```bash
mkdir -p "$HOME/projects/aura-hello"
cd "$HOME/projects/aura-hello"
printf '%s\n' 'print("hello from Aura in WSL")' > hello.au
aura run hello.au
```

Windows drives are available under paths such as `/mnt/c`, but the Linux home
directory is the recommended location for WSL development projects.

## 6. Connect VS Code To WSL

Install Visual Studio Code on Windows and select **Add to PATH** in its Windows
installer. Install Microsoft's **WSL** extension in the local VS Code window.

From the Ubuntu terminal, open the project:

```bash
cd "$HOME/projects/aura-hello"
code .
```

VS Code installs its server inside WSL and opens a remote window. The status
bar must show **WSL: Ubuntu**. In that remote window, install
**Aura Programming Language** into WSL. A locally installed copy is not enough
because the extension must launch the `aura lsp` executable inside Ubuntu.

Continue with the complete [VS Code extension guide](/install/vscode).

## Upgrade Aura

Run the updater from the Ubuntu terminal:

```bash
aura upgrade
aura --version
```

The command upgrades the Linux compiler and runtime inside WSL. Run it in the
Ubuntu terminal, not PowerShell.

## Troubleshooting

### `wsl --install` displays help

WSL may already be present. Run `wsl --list --online`, then install the exact
Ubuntu distribution name shown by that command.

### `code` is not found inside Ubuntu

Install VS Code on Windows with its **Add to PATH** option, close the Ubuntu
terminal, reopen it, and run `code .` again.

### VS Code cannot find `aura`

Open a terminal in the **WSL: Ubuntu** window and run:

```bash
command -v aura
aura --version
```

If the commands fail, restore the PATH export from step 4 and restart the WSL
VS Code window. Do not install a Windows copy of Aura; the current compiler
distribution is the Linux binary running inside WSL.

### Windows on ARM

The current Aura release has no Linux ARM64 archive. Windows-on-ARM WSL hosts
are outside the supported distribution matrix for this preview.
