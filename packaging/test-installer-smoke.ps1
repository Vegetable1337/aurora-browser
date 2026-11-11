param(
    [string]$SetupPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'dist\alpha\Aurora-0.1.0-alpha-Windows-x64-Setup.exe')
)

$ErrorActionPreference = 'Stop'
$setup = (Resolve-Path -LiteralPath $SetupPath).Path
$tempRoot = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
$installDir = Join-Path $tempRoot ('AuroraInstallerSmoke-' + [guid]::NewGuid().ToString('N'))
$shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Aurora Browser\Aurora Browser.lnk'
$uninstallRoot = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
$appIdKey = '{F8D3D434-26DD-4D95-89BE-FC673DFD08EE}_is1'
$uninstaller = $null
$existingKey = Join-Path $uninstallRoot $appIdKey

if (Get-Process -Name 'aurora-browser' -ErrorAction SilentlyContinue) {
    throw 'Close Aurora before running the installer smoke test; the test will not close an existing browser.'
}
if (Test-Path -LiteralPath $existingKey) {
    throw 'An Aurora installation already exists for this Windows user; refusing to overwrite it during the disposable smoke test.'
}
if (Test-Path -LiteralPath $installDir) {
    throw "Generated test directory unexpectedly already exists: $installDir"
}

function Invoke-Installer([string]$Path, [string[]]$Arguments) {
    $process = Start-Process -FilePath $Path -ArgumentList $Arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "Installer process '$Path' exited with code $($process.ExitCode)."
    }
}

function Find-StartMenuShortcut {
    $matching = @(Get-ChildItem -LiteralPath (Split-Path -Parent $shortcut) -Filter '*.lnk' -ErrorAction SilentlyContinue)
    return $matching | Where-Object { $_.Name -eq 'Aurora Browser.lnk' } | Select-Object -First 1
}

try {
    $installArguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', "/DIR=$installDir")
    Invoke-Installer $setup $installArguments

    $exe = Join-Path $installDir 'Aurora.exe'
    $uninstaller = Join-Path $installDir 'unins000.exe'
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Installed Aurora.exe was not found.' }
    if (-not (Test-Path -LiteralPath $uninstaller -PathType Leaf)) { throw 'Installed uninstaller was not found.' }
    foreach ($notice in @('LICENSE', 'THIRD_PARTY_NOTICES.md', 'third_party\licenses\MPL-2.0.txt')) {
        if (-not (Test-Path -LiteralPath (Join-Path $installDir $notice) -PathType Leaf)) {
            throw "Installed third-party license notice missing: $notice"
        }
    }
    if (-not (Test-Path -LiteralPath $existingKey)) { throw 'Windows uninstall registration was not created.' }
    if ($null -eq (Find-StartMenuShortcut)) { throw 'Start menu shortcut was not created.' }

    $version = (Get-Item -LiteralPath $exe).VersionInfo
    if ($version.ProductVersion -ne '0.1.0-alpha') {
        throw "Installed executable has unexpected version '$($version.ProductVersion)'."
    }

    Invoke-Installer $setup $installArguments
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Aurora.exe disappeared during repeat installation.' }
    if (-not (Test-Path -LiteralPath $existingKey)) { throw 'Uninstall registration disappeared during repeat installation.' }

    Invoke-Installer $uninstaller @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART')
    $uninstaller = $null
    if (Test-Path -LiteralPath $installDir) { throw 'Test installation directory remains after uninstall.' }
    if (Test-Path -LiteralPath $existingKey) { throw 'Windows uninstall registration remains after uninstall.' }
    if (Test-Path -LiteralPath $shortcut) { throw 'Start menu shortcut remains after uninstall.' }

    $profileRoot = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Aurora\Rust'))
    $installRootFull = [IO.Path]::GetFullPath($installDir).TrimEnd('\') + '\'
    $profileIsSeparate = -not $profileRoot.StartsWith($installRootFull, [StringComparison]::OrdinalIgnoreCase)
    if (-not $profileIsSeparate) { throw 'The user profile directory overlaps the temporary test installation.' }
    [pscustomobject]@{
        InstallerVersion = $version.ProductVersion
        FirstInstall = 'passed'
        RepeatInstall = 'passed'
        StartMenuShortcut = 'passed'
        WindowsUninstallRegistration = 'passed'
        Uninstall = 'passed'
        UserDataRootSeparateFromInstall = $profileIsSeparate
        ProfileDataPreservation = 'Not exercised against an isolated Windows user profile.'
        Scope = 'Disposable custom install directory under TEMP; no browser process was running.'
    } | ConvertTo-Json -Depth 3
}
finally {
    if ($null -ne $uninstaller -and (Test-Path -LiteralPath $uninstaller)) {
        try { Invoke-Installer $uninstaller @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') }
        catch { Write-Warning "Could not invoke the disposable installation's uninstaller: $($_.Exception.Message)" }
    }

    $resolvedInstallDir = [IO.Path]::GetFullPath($installDir)
    if (-not $resolvedInstallDir.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to remove any path outside TEMP.'
    }
    if ((Test-Path -LiteralPath $existingKey) -and (Test-Path -LiteralPath $resolvedInstallDir)) {
        Write-Warning "The disposable app is still registered; leaving its folder intact for its uninstaller: $resolvedInstallDir"
    }
    elseif (Test-Path -LiteralPath $resolvedInstallDir) {
        Remove-Item -LiteralPath $resolvedInstallDir -Recurse -Force
    }
}
