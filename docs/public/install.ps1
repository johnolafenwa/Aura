$ErrorActionPreference = 'Stop'

# Everything runs from Install-Aura, called on the last line, so a download
# cut off partway through `irm ... | iex` defines code but runs none of it.
function Install-Aura {
    $releaseTag = if ($env:AURA_INSTALL_RELEASE_TAG) { $env:AURA_INSTALL_RELEASE_TAG } else { 'v0.14.0-preview' }
    $prefix = if ($env:AURA_INSTALL_PREFIX) { $env:AURA_INSTALL_PREFIX } else { Join-Path $env:LOCALAPPDATA 'Programs\Aura' }
    $baseUrl = if ($env:AURA_INSTALL_BASE_URL) { $env:AURA_INSTALL_BASE_URL } else { "https://github.com/johnolafenwa/Aura/releases/download/$releaseTag" }

    function Fail([string]$message) {
        throw "Aura installer: $message"
    }

    if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64' -or $env:PROCESSOR_ARCHITEW6432 -eq 'ARM64') {
        Fail "no release archive is available for Windows on $env:PROCESSOR_ARCHITECTURE"
    }
    if ($baseUrl -notmatch '^(https|file)://') {
        Fail "refusing insecure AURA_INSTALL_BASE_URL '$baseUrl'; expected an https:// URL"
    }

    $archiveRoot = "aura-$releaseTag-x86_64-pc-windows-msvc"
    $archive = "$archiveRoot.zip"
    $work = Join-Path ([System.IO.Path]::GetTempPath()) ("aura-install-" + [System.Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $work | Out-Null
    try {
        Write-Host "Downloading Aura $releaseTag for x86_64-pc-windows-msvc..."
        $archivePath = Join-Path $work $archive
        $sumsPath = Join-Path $work 'SHA256SUMS'
        try {
            foreach ($download in @(@($archive, $archivePath), @('SHA256SUMS', $sumsPath))) {
                $url = "$baseUrl/$($download[0])"
                if ($url -match '^file://') {
                    Copy-Item -LiteralPath ([System.Uri]$url).LocalPath -Destination $download[1]
                } else {
                    Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $download[1]
                }
            }
        } catch {
            Fail "could not download $archive and SHA256SUMS: $($_.Exception.Message)"
        }

        $expected = $null
        foreach ($line in Get-Content -LiteralPath $sumsPath) {
            $fields = $line -split '\s+', 2
            if ($fields.Count -eq 2 -and $fields[1].TrimStart('*') -eq $archive) {
                $expected = $fields[0]
            }
        }
        if (-not $expected) {
            Fail "$archive is missing from SHA256SUMS"
        }
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash
        if ($actual -ne $expected) {
            Fail 'checksum verification failed'
        }

        Expand-Archive -LiteralPath $archivePath -DestinationPath $work
        $source = Join-Path $work $archiveRoot
        $runtime = Join-Path $source 'lib\aura'
        if (-not (Test-Path -LiteralPath (Join-Path $source 'bin\aura.exe') -PathType Leaf)) {
            Fail 'the archive does not contain bin\aura.exe'
        }
        if (-not (Test-Path -LiteralPath (Join-Path $runtime 'aura_runtime.lib') -PathType Leaf)) {
            Fail 'the archive is missing the native runtime'
        }
        if (-not (Test-Path -LiteralPath (Join-Path $runtime 'native-link-args.json') -PathType Leaf)) {
            Fail 'the archive is missing native-link-args.json'
        }

        $bin = Join-Path $prefix 'bin'
        $lib = Join-Path $prefix 'lib\aura'
        $share = Join-Path $prefix 'share\aura'
        New-Item -ItemType Directory -Force -Path $bin, $lib, $share | Out-Null
        Copy-Item -LiteralPath (Join-Path $source 'bin\aura.exe') -Destination (Join-Path $bin 'aura.exe') -Force
        # The runtime and the import libraries it names are replaced as a set.
        Get-ChildItem -LiteralPath $lib -Filter '*.lib' | Remove-Item -Force
        Copy-Item -Path (Join-Path $runtime '*') -Destination $lib -Force
        $examples = Join-Path $source 'examples'
        if (Test-Path -LiteralPath $examples -PathType Container) {
            $installedExamples = Join-Path $share 'examples'
            if (Test-Path -LiteralPath $installedExamples) {
                Remove-Item -LiteralPath $installedExamples -Recurse -Force
            }
            Copy-Item -LiteralPath $examples -Destination $installedExamples -Recurse
        }
        foreach ($file in 'README.md', 'AURA_CLI_README.md', 'LICENSE') {
            $path = Join-Path $source $file
            if (Test-Path -LiteralPath $path -PathType Leaf) {
                Copy-Item -LiteralPath $path -Destination (Join-Path $share $file) -Force
            }
        }

        Write-Host ''
        Write-Host "Aura is installed at $bin\aura.exe"
        if (($env:PATH -split ';') -contains $bin) {
            Write-Host 'Run: aura --version'
        } else {
            Write-Host 'Add Aura to this shell with:'
            Write-Host "  `$env:PATH = `"$bin;`$env:PATH`""
        }
        Write-Host 'aura build and aura run link programs with the Microsoft linker.'
        Write-Host 'Install the Visual Studio Build Tools with the "Desktop development with C++" workload.'
    } finally {
        Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Install-Aura
