param(
    [string]$IsccPath = 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe'
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$releaseExe = Join-Path $repoRoot 'rust-browser\target\release\aurora-browser.exe'
$alphaDir = Join-Path $repoRoot 'dist\alpha'
$installerScript = Join-Path $PSScriptRoot 'aurora.iss'
$readme = Join-Path $repoRoot 'docs\alpha\README.txt'
$license = Join-Path $repoRoot 'LICENSE'
$zipPath = Join-Path $alphaDir 'Aurora-0.1.0-alpha-Windows-x64-Portable.zip'
$hashPath = Join-Path $alphaDir 'SHA256SUMS.txt'

foreach ($required in @($releaseExe, $installerScript, $readme, $license, $IsccPath)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Required packaging input is missing: $required"
    }
}

New-Item -ItemType Directory -Path $alphaDir -Force | Out-Null
$installerPath = Join-Path $alphaDir 'Aurora-0.1.0-alpha-Windows-x64-Setup.exe'
# Clear only artifacts produced by this packaging step, so a failed run cannot
# leave a stale installer or checksum manifest looking current.
foreach ($output in @($installerPath, $zipPath, $hashPath)) {
    if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Force }
}
$stage = Join-Path $alphaDir ('.stage-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null

try {
    Copy-Item -LiteralPath $releaseExe -Destination (Join-Path $stage 'Aurora.exe')
    Copy-Item -LiteralPath $readme -Destination (Join-Path $stage 'README.txt')
    Copy-Item -LiteralPath $license -Destination (Join-Path $stage 'LICENSE')

    $version = (Get-Item -LiteralPath (Join-Path $stage 'Aurora.exe')).VersionInfo
    if ($version.ProductName -ne 'Aurora Browser' -or
        $version.ProductVersion -ne '0.1.0-alpha' -or
        $version.OriginalFilename -ne 'Aurora.exe') {
        throw "Executable version metadata is invalid (product='$($version.ProductName)', version='$($version.ProductVersion)', filename='$($version.OriginalFilename)')."
    }

    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zipPath -CompressionLevel Optimal
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $portable = [System.IO.Compression.ZipFile]::OpenRead($zipPath)
    try {
        $entries = @($portable.Entries | ForEach-Object FullName | Sort-Object)
        if (($entries -join '|') -ne 'Aurora.exe|LICENSE|README.txt') {
            throw "Unexpected portable ZIP contents: $($entries -join ', ')"
        }
    }
    finally { $portable.Dispose() }

    $env:AURORA_ALPHA_PAYLOAD = $stage
    & $IsccPath $installerScript
    Remove-Item Env:\AURORA_ALPHA_PAYLOAD -ErrorAction SilentlyContinue
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup compiler failed with exit code $LASTEXITCODE." }

    if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) {
        throw "Installer output was not created: $installerPath"
    }

    $artifacts = @($installerPath, $zipPath)
    $lines = foreach ($artifact in $artifacts) {
        $hash = Get-FileHash -LiteralPath $artifact -Algorithm SHA256
        '{0}  {1}' -f $hash.Hash.ToLowerInvariant(), (Split-Path -Leaf $artifact)
    }
    Set-Content -LiteralPath $hashPath -Value $lines -Encoding ascii
    foreach ($artifact in $artifacts) {
        $expected = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
        if (-not (Select-String -LiteralPath $hashPath -SimpleMatch $expected -Quiet)) {
            throw "SHA-256 manifest verification failed for $artifact"
        }
    }
    Write-Host "Created installer, portable ZIP, and SHA-256 manifest in $alphaDir"
}
finally {
    $resolvedAlphaDir = [System.IO.Path]::GetFullPath($alphaDir)
    $resolvedStage = [System.IO.Path]::GetFullPath($stage)
    if (-not $resolvedStage.StartsWith($resolvedAlphaDir, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to remove staging path outside dist/alpha.'
    }
    Remove-Item -LiteralPath $resolvedStage -Recurse -Force
}
