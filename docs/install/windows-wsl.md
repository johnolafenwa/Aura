# Install Aura On Windows With WSL

Aura has no native Windows executable. On an x86-64 Windows 11 machine, you
install and run the Linux release inside Windows Subsystem for Linux 2
(WSL 2) with Ubuntu 24.04. The CLI, compiler runtime, your projects, and the
VS Code language server all run inside WSL 2.

## 1. Install WSL 2 And Ubuntu

Open PowerShell as Administrator. List the available distribution names:

```powershell
wsl --list --online
```

Install Ubuntu 24.04:

```powershell
wsl --install -d Ubuntu-24.04
```

Restart Windows if asked. Launch **Ubuntu 24.04 LTS** from the Start menu.
On first launch, create the Linux username and password it asks for.

For older Windows builds and existing WSL setups, Microsoft's
[WSL installation guide](https://learn.microsoft.com/windows/wsl/install)
documents recovery steps.

## 2. Confirm WSL 2

In PowerShell:

```powershell
wsl --list --verbose
```

The Ubuntu row must show version `2`. If it shows version `1`, convert it.
Use the exact distribution name that the previous command printed:

```powershell
wsl --set-version Ubuntu-24.04 2
```

Run every remaining command in this guide in the Ubuntu terminal, not
PowerShell.

## 3. Prepare Ubuntu

```bash
sudo apt update
sudo apt install -y curl ca-certificates tar coreutils build-essential
```

The first four packages let you install and verify Aura. `build-essential`
enables direct native execution and `aura build`.

## 4. Install Aura Inside WSL

Run the installer:

```bash
curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh
```

Add Aura to the Ubuntu login environment:

```bash
grep -qxF 'export PATH="$HOME/.local/bin:$PATH"' "$HOME/.profile" || \
  printf '%s\n' 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.profile"
export PATH="$HOME/.local/bin:$PATH"
```

Verify both the CLI and the native toolchain:

```bash
aura --version
cc --version
```

## 5. Create A Project In The WSL Filesystem

Keep active Aura projects under your Linux home directory. There, Linux tools
get normal permissions and filesystem behavior:

```bash
mkdir -p "$HOME/projects/aura-hello"
cd "$HOME/projects/aura-hello"
printf '%s\n' 'print("hello from Aura in WSL")' > hello.au
aura run hello.au
```

Windows drives are available under paths such as `/mnt/c`. The Linux home
directory is still the recommended place for projects you develop in WSL.

## 6. Connect VS Code To WSL

1. Install Visual Studio Code on Windows. In its installer, select
   **Add to PATH**.
2. In the local VS Code window, install Microsoft's **WSL** extension.
3. From the Ubuntu terminal, open the project:

```bash
cd "$HOME/projects/aura-hello"
code .
```

4. VS Code installs its server inside WSL and opens a remote window. Check
   that the status bar shows **WSL: Ubuntu**.
5. In that remote window, install **Aura Programming Language** into WSL.

A copy of the extension installed only locally is not enough. The extension
must launch the `aura lsp` executable inside Ubuntu.

Continue with the complete [VS Code extension guide](/install/vscode).

## Upgrade Aura

Run the updater from the Ubuntu terminal, not PowerShell:

```bash
aura upgrade
aura --version
```

This upgrades the Linux compiler and runtime inside WSL.

## Troubleshooting

### `wsl --install` displays help

WSL may already be installed. Run `wsl --list --online`, then install the
exact Ubuntu distribution name that command shows.

### `code` is not found inside Ubuntu

Install VS Code on Windows with its **Add to PATH** option. Close the Ubuntu
terminal, reopen it, and run `code .` again.

### VS Code cannot find `aura`

Open a terminal in the **WSL: Ubuntu** window and run:

```bash
command -v aura
aura --version
```

If these commands fail, restore the `PATH` export from step 4 and restart the
WSL VS Code window. Do not install a Windows copy of Aura. The current
compiler ships only as the Linux binary running inside WSL.

### Windows on ARM

The current Aura release has no Linux ARM64 archive. Windows-on-ARM WSL hosts
are not supported in this preview.
