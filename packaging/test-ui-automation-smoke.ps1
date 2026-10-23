param(
    [string]$ExePath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'rust-browser\target\release\aurora-browser.exe'),
    [int]$TimeoutSeconds = 30
)

$ErrorActionPreference = 'Stop'
[void][Reflection.Assembly]::Load('UIAutomationClient')
[void][Reflection.Assembly]::Load('UIAutomationTypes')

$exe = (Resolve-Path -LiteralPath $ExePath).Path
$tempRoot = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
$sandbox = Join-Path $tempRoot ('AuroraUiAutomation-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $sandbox | Out-Null

$environmentValues = @{
    LOCALAPPDATA = Join-Path $sandbox 'LocalAppData'
    APPDATA = Join-Path $sandbox 'Roaming'
    TEMP = $sandbox
    TMP = $sandbox
    PATH = "$env:SystemRoot\System32;$env:SystemRoot;$env:SystemRoot\System32\Wbem"
}
$oldEnvironment = @{}
$process = $null

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
    $window = $null
    do {
        $process.Refresh()
        if ($process.HasExited) {
            throw "Aurora exited before becoming accessible (exit $($process.ExitCode))."
        }
        $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
            [System.Windows.Automation.TreeScope]::Children,
            $condition
        )
        foreach ($candidate in $windows) {
            if ($candidate.Current.ControlType -eq [System.Windows.Automation.ControlType]::Window -and
                $candidate.Current.Name -eq 'Aurora' -and
                -not $candidate.Current.IsOffscreen) {
                $window = $candidate
                break
            }
        }
        if ($null -eq $window) { Start-Sleep -Milliseconds 250 }
    } while ($null -eq $window -and [DateTime]::UtcNow -lt $deadline)

    if ($null -eq $window) {
        throw "Windows UI Automation did not expose a visible Aurora window within $TimeoutSeconds seconds."
    }

    $controls = $window.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.Condition]::TrueCondition
    )
    $summary = foreach ($control in $controls) {
        $invokePattern = $null
        $hasInvoke = $control.TryGetCurrentPattern(
            [System.Windows.Automation.InvokePattern]::Pattern,
            [ref]$invokePattern
        )
        [pscustomobject]@{
            Type = $control.Current.ControlType.ProgrammaticName
            Name = $control.Current.Name
            HasInvoke = $hasInvoke
        }
    }
    [pscustomobject]@{
        ProcessId = $process.Id
        WindowName = $window.Current.Name
        WindowOffscreen = $window.Current.IsOffscreen
        ControlCount = $summary.Count
        Controls = @($summary)
    } | ConvertTo-Json -Depth 5
}
finally {
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
        throw 'Refusing to remove UI Automation test data outside TEMP.'
    }
    if (Test-Path -LiteralPath $resolvedSandbox) {
        $removed = $false
        for ($attempt = 0; $attempt -lt 10 -and -not $removed; $attempt++) {
            try {
                Remove-Item -LiteralPath $resolvedSandbox -Recurse -Force -ErrorAction Stop
                $removed = $true
            }
            catch {
                Start-Sleep -Milliseconds 300
            }
        }
        if (-not $removed) {
            Write-Warning "Could not remove the disposable UI Automation profile; it is retained at $resolvedSandbox"
        }
    }
}
