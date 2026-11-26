$ErrorActionPreference = 'Stop'

# Everything runs from Install-Aura, called on the last line, so a download
# cut off partway through `irm ... | iex` defines code but runs none of it.
function Install-Aura {
    $releaseTag = if ($env:AURA_INSTALL_RELEASE_TAG) { $env:AURA_INSTALL_RELEASE_TAG } else { 'v0.17.0-preview' }
    $prefix = if ($env:AURA_INSTALL_PREFIX) { $env:AURA_INSTALL_PREFIX } else { Join-Path $env:LOCALAPPDATA 'Programs\Aura' }
    $baseUrl = if ($env:AURA_INSTALL_BASE_URL) { $env:AURA_INSTALL_BASE_URL } else { "https://github.com/johnolafenwa/Aura/releases/download/$releaseTag" }

    function Fail([string]$message) {
        throw "Aura installer: $message"
    }

    if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64' -or $env:PROCESSOR_ARCHITEW6432 -eq 'ARM64') {
        Fail "no release archive is available for Windows on $env:PROCESSOR_ARCHITECTURE"
    }
    if ($releaseTag -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$' -or $releaseTag.Contains('..')) {
        Fail "invalid AURA_INSTALL_RELEASE_TAG '$releaseTag'"
    }
    if ($baseUrl -notmatch '^(https|file)://') {
        Fail "refusing insecure AURA_INSTALL_BASE_URL '$baseUrl'; expected an https:// URL"
    }
    $baseUrl = $baseUrl.TrimEnd('/')

    $tlsProtocols = [Net.SecurityProtocolType]::Tls12
    if ([Enum]::IsDefined([Net.SecurityProtocolType], 'Tls13')) {
        $tlsProtocols = $tlsProtocols -bor [Net.SecurityProtocolType]::Tls13
    }
    [Net.ServicePointManager]::SecurityProtocol = $tlsProtocols

    Add-Type -AssemblyName System.Net.Http
    Add-Type -AssemblyName System.IO.Compression.FileSystem

    function Download-HttpsFile([string]$uri, [string]$destination) {
        $handler = New-Object System.Net.Http.HttpClientHandler
        $handler.AllowAutoRedirect = $false
        if ($handler.PSObject.Properties['SslProtocols']) {
            $sslProtocols = [System.Security.Authentication.SslProtocols]::Tls12
            if ([Enum]::IsDefined([System.Security.Authentication.SslProtocols], 'Tls13')) {
                $sslProtocols = $sslProtocols -bor [System.Security.Authentication.SslProtocols]::Tls13
            }
            $handler.SslProtocols = $sslProtocols
        }
        $client = New-Object System.Net.Http.HttpClient($handler, $false)
        try {
            $client.DefaultRequestHeaders.UserAgent.ParseAdd('aura-installer')
            $currentUri = [System.Uri]$uri
            for ($hop = 0; $hop -le 10; $hop++) {
                if ($currentUri.Scheme -ne 'https') {
                    Fail "refusing non-HTTPS download URL '$currentUri'"
                }
                $response = $client.GetAsync($currentUri, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
                try {
                    $statusCode = [int]$response.StatusCode
                    if ($statusCode -in @(301, 302, 303, 307, 308)) {
                        if ($hop -eq 10) {
                            Fail "too many redirects while downloading '$uri'"
                        }
                        $location = $response.Headers.Location
                        if ($null -eq $location) {
                            Fail "redirect response from '$currentUri' is missing a Location header"
                        }
                        $nextUri = if ($location.IsAbsoluteUri) { $location } else { New-Object System.Uri($currentUri, $location) }
                        if ($nextUri.Scheme -ne 'https') {
                            Fail "refusing non-HTTPS redirect to '$nextUri'"
                        }
                        $currentUri = $nextUri
                        continue
                    }
                    $response.EnsureSuccessStatusCode() | Out-Null
                    $inStream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
                    try {
                        $outStream = [System.IO.File]::Create($destination)
                        try {
                            $inStream.CopyTo($outStream)
                        } finally {
                            $outStream.Dispose()
                        }
                    } finally {
                        $inStream.Dispose()
                    }
                    return
                } finally {
                    $response.Dispose()
                }
            }
        } finally {
            $client.Dispose()
            $handler.Dispose()
        }
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
                    Download-HttpsFile -uri $url -destination $download[1]
                }
            }
        } catch {
            Fail "could not download $archive and SHA256SUMS: $($_.Exception.Message)"
        }

        $matchesFound = @()
        foreach ($line in Get-Content -LiteralPath $sumsPath) {
            if ($line -match '^\s*$') {
                continue
            }
            $fields = $line -split '\s+', 2
            if ($fields.Count -ne 2) {
                Fail 'invalid entry in SHA256SUMS'
            }
            $digest = $fields[0]
            $entryName = $fields[1]
            if ($entryName.StartsWith('*')) {
                $entryName = $entryName.Substring(1)
            }
            if ($entryName -eq $archive) {
                if ($digest -notmatch '^[0-9A-Fa-f]{64}$') {
                    Fail "invalid SHA-256 digest for $archive in SHA256SUMS"
                }
                $matchesFound += $digest
            }
        }
        if ($matchesFound.Count -eq 0) {
            Fail "$archive is missing from SHA256SUMS"
        }
        if ($matchesFound.Count -gt 1) {
            Fail "duplicate SHA256SUMS entries for $archive"
        }
        $expected = $matchesFound[0]
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash
        if ($actual -ne $expected) {
            Fail 'checksum verification failed'
        }

        $extractDir = Join-Path $work 'extract'
        New-Item -ItemType Directory -Path $extractDir | Out-Null
        $expectedRoot = [System.IO.Path]::GetFullPath((Join-Path $extractDir $archiveRoot))
        $expectedPrefix = $expectedRoot.TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar

        $zip = [System.IO.Compression.ZipFile]::OpenRead($archivePath)
        try {
            foreach ($entry in $zip.Entries) {
                $entryName = $entry.FullName
                if ([string]::IsNullOrEmpty($entryName) -or $entryName -match '^[\\/]|:' -or (($entryName -split '[\\/]') -contains '..')) {
                    Fail "invalid archive entry '$entryName'"
                }
                if ((($entry.ExternalAttributes -shr 16) -band 0xF000) -eq 0xA000) {
                    Fail "archive entry '$entryName' is a symbolic link"
                }
                $relativePath = $entryName -replace '/', '\'
                $destinationPath = [System.IO.Path]::GetFullPath((Join-Path $extractDir $relativePath))
                $isRootDir = $destinationPath.Equals($expectedRoot, [System.StringComparison]::OrdinalIgnoreCase)
                $isInsideRoot = $destinationPath.StartsWith($expectedPrefix, [System.StringComparison]::OrdinalIgnoreCase)
                if ($entryName.EndsWith('/') -or $entryName.EndsWith('\')) {
                    if (-not ($isRootDir -or $isInsideRoot)) {
                        Fail "archive entry '$entryName' escapes $archiveRoot"
                    }
                    New-Item -ItemType Directory -Force -Path $destinationPath | Out-Null
                } else {
                    if (-not $isInsideRoot) {
                        Fail "archive entry '$entryName' escapes $archiveRoot"
                    }
                    $parent = Split-Path -Parent $destinationPath
                    if ($parent) {
                        New-Item -ItemType Directory -Force -Path $parent | Out-Null
                    }
                    $inStream = $entry.Open()
                    try {
                        $outStream = [System.IO.File]::Create($destinationPath)
                        try {
                            $inStream.CopyTo($outStream)
                        } finally {
                            $outStream.Dispose()
                        }
                    } finally {
                        $inStream.Dispose()
                    }
                }
            }
        } finally {
            $zip.Dispose()
        }

        $source = $expectedRoot
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
        Get-ChildItem -LiteralPath $lib -Filter '*.lib' -File | Remove-Item -Force
        foreach ($runtimeItem in Get-ChildItem -LiteralPath $runtime -File) {
            Copy-Item -LiteralPath $runtimeItem.FullName -Destination (Join-Path $lib $runtimeItem.Name) -Force
        }
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
