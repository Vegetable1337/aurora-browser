param(
    [string]$ExePath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'rust-browser\target\release\aurora-browser.exe'),
    [string]$OutputPath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'docs\alpha\evidence\main-window.png'),
    [int]$TimeoutSeconds = 30,
    [int]$StableDelaySeconds = 0,
    [ValidateSet('dark', 'light')]
    [string]$Theme = 'dark'
)

$ErrorActionPreference = 'Stop'
[void][Reflection.Assembly]::Load('UIAutomationClient')
[void][Reflection.Assembly]::Load('UIAutomationTypes')
[void][Reflection.Assembly]::Load('System.Drawing')
[void][Reflection.Assembly]::Load('System.Windows.Forms')

$exe = (Resolve-Path -LiteralPath $ExePath).Path
$tempRoot = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
$sandbox = Join-Path $tempRoot ('AuroraVisualSmoke-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $sandbox | Out-Null
$output = [IO.Path]::GetFullPath($OutputPath)
New-Item -ItemType Directory -Path (Split-Path -Parent $output) -Force | Out-Null

$environmentValues = @{
    LOCALAPPDATA = Join-Path $sandbox 'LocalAppData'
    APPDATA = Join-Path $sandbox 'Roaming'
    TEMP = $sandbox
    TMP = $sandbox
    PATH = "$env:SystemRoot\System32;$env:SystemRoot;$env:SystemRoot\System32\Wbem"
}
$settingsDirectory = Join-Path $environmentValues.LOCALAPPDATA 'Aurora\Rust\Default'
New-Item -ItemType Directory -Path $settingsDirectory -Force | Out-Null
@{
    search_engine = 'duckduckgo'
    animations = $true
    theme = $Theme
    homepage = 'aurora'
    adblock_enabled = $true
    https_first = $true
} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $settingsDirectory 'settings.json') -Encoding utf8
$oldEnvironment = @{}
$process = $null
$window = $null
$bitmap = $null
$graphics = $null

try {
    foreach ($name in $environmentValues.Keys) {
        $oldEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        [Environment]::SetEnvironmentVariable($name, $environmentValues[$name], 'Process')
    }
    $process = Start-Process -FilePath $exe -WorkingDirectory $sandbox -PassThru
    foreach ($name in $environmentValues.Keys) {
        [Environment]::SetEnvironmentVariable($name, $oldEnvironment[$name], 'Process')
    }

    $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
        $process.Id
    )
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $process.Refresh()
        if ($process.HasExited) { throw "Aurora exited before the window appeared (exit $($process.ExitCode))." }
        $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
            [System.Windows.Automation.TreeScope]::Children,
            $condition
        )
        foreach ($candidate in $windows) {
            if ($candidate.Current.Name -eq 'Aurora' -and -not $candidate.Current.IsOffscreen) {
                $window = $candidate
                break
            }
        }
        if ($null -eq $window) { Start-Sleep -Milliseconds 250 }
    } while ($null -eq $window -and [DateTime]::UtcNow -lt $deadline)

    if ($null -eq $window) { throw "No visible Aurora window appeared within $TimeoutSeconds seconds." }
    if ($StableDelaySeconds -gt 0) { Start-Sleep -Seconds $StableDelaySeconds }

    $bounds = $window.Current.BoundingRectangle
    $windowX = [int][Math]::Floor($bounds.X)
    $windowY = [int][Math]::Floor($bounds.Y)
    $windowRight = [int][Math]::Ceiling($bounds.Right)
    $windowBottom = [int][Math]::Ceiling($bounds.Bottom)
    $monitor = [System.Windows.Forms.Screen]::AllScreens |
        Sort-Object { [Math]::Max(0, [Math]::Min($_.Bounds.Right, $windowRight) - [Math]::Max($_.Bounds.Left, $windowX)) *
                      [Math]::Max(0, [Math]::Min($_.Bounds.Bottom, $windowBottom) - [Math]::Max($_.Bounds.Top, $windowY)) } -Descending |
        Select-Object -First 1
    $x = [Math]::Max($monitor.Bounds.Left, $windowX)
    $y = [Math]::Max($monitor.Bounds.Top, $windowY)
    $right = [Math]::Min($monitor.Bounds.Right, $windowRight)
    $bottom = [Math]::Min($monitor.Bounds.Bottom, $windowBottom)
    $width = $right - $x
    $height = $bottom - $y
    if ($width -le 0 -or $height -le 0) {
        throw "Aurora window bounds are invalid: $bounds"
    }

    $bitmap = [Drawing.Bitmap]::new($width, $height)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($x, $y, 0, 0, [Drawing.Size]::new($width, $height), [Drawing.CopyPixelOperation]::SourceCopy)
    $bitmap.Save($output, [Drawing.Imaging.ImageFormat]::Png)
    [pscustomobject]@{
        WindowName = $window.Current.Name
        WindowBounds = @{ X = $windowX; Y = $windowY; Right = $windowRight; Bottom = $windowBottom }
        CaptureBounds = @{ X = $x; Y = $y; Width = $width; Height = $height }
        Screenshot = $output
        IsOffscreen = $window.Current.IsOffscreen
        RustToolchainAvailableToProcess = $false
        Profile = 'Disposable temporary profile'
        Theme = $Theme
    } | ConvertTo-Json -Depth 3
}
finally {
    if ($null -ne $graphics) { $graphics.Dispose() }
    if ($null -ne $bitmap) { $bitmap.Dispose() }
    foreach ($name in $environmentValues.Keys) {
        [Environment]::SetEnvironmentVariable($name, $oldEnvironment[$name], 'Process')
    }
    if ($null -ne $process) {
        $process.Refresh()
        if (-not $process.HasExited -and $null -ne $window) {
            try { $window.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close() }
            catch { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
        }
        if (-not $process.WaitForExit(15000)) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
            $process.WaitForExit(5000) | Out-Null
        }
    }

    $resolvedSandbox = [IO.Path]::GetFullPath($sandbox)
    if (-not $resolvedSandbox.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to remove visual smoke data outside TEMP.'
    }
    if (Test-Path -LiteralPath $resolvedSandbox) {
        $removed = $false
        for ($attempt = 0; $attempt -lt 10 -and -not $removed; $attempt++) {
            try {
                Remove-Item -LiteralPath $resolvedSandbox -Recurse -Force -ErrorAction Stop
                $removed = $true
            }
            catch { Start-Sleep -Milliseconds 300 }
        }
        if (-not $removed) { Write-Warning "Disposable profile retained at $resolvedSandbox" }
    }
}
