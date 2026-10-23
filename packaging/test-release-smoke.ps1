param(
    [string]$ExePath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'rust-browser\target\release\aurora-browser.exe')
)

$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $ExePath).Path
$tempRoot = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
$sandbox = Join-Path $tempRoot ('AuroraReleaseSmoke-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $sandbox | Out-Null

Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public sealed class AuroraWindowInfo {
    public IntPtr Handle;
    public uint ProcessId;
    public bool Visible;
    public string ClassName;
    public string Title;
}

public static class AuroraReleaseWindowProbe {
    private delegate bool EnumWindowsCallback(IntPtr handle, IntPtr state);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsCallback callback, IntPtr state);
    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr handle, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetClassName(IntPtr handle, StringBuilder value, int maxCount);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr handle, StringBuilder value, int maxCount);
    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr handle);
    [DllImport("user32.dll")]
    private static extern bool PostMessage(IntPtr handle, uint message, IntPtr wParam, IntPtr lParam);

    public static AuroraWindowInfo[] GetWindows(uint targetProcessId) {
        var result = new List<AuroraWindowInfo>();
        EnumWindowsCallback callback = (handle, _) => {
            uint owner;
            GetWindowThreadProcessId(handle, out owner);
            if (owner == targetProcessId) {
                var className = new StringBuilder(256);
                var title = new StringBuilder(512);
                GetClassName(handle, className, className.Capacity);
                GetWindowText(handle, title, title.Capacity);
                result.Add(new AuroraWindowInfo {
                    Handle = handle,
                    ProcessId = owner,
                    Visible = IsWindowVisible(handle),
                    ClassName = className.ToString(),
                    Title = title.ToString()
                });
            }
            return true;
        };
        EnumWindows(callback, IntPtr.Zero);
        GC.KeepAlive(callback);
        return result.ToArray();
    }

    public static bool RequestClose(IntPtr handle) {
        return PostMessage(handle, 0x0010, IntPtr.Zero, IntPtr.Zero);
    }
}
'@

$process = $null
$oldEnvironment = @{}
$environmentValues = @{
    LOCALAPPDATA = Join-Path $sandbox 'LocalAppData'
    APPDATA = Join-Path $sandbox 'Roaming'
    TEMP = $sandbox
    TMP = $sandbox
    PATH = "$env:SystemRoot\System32;$env:SystemRoot;$env:SystemRoot\System32\Wbem"
}

try {
    foreach ($name in $environmentValues.Keys) {
        $oldEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        [Environment]::SetEnvironmentVariable($name, $environmentValues[$name], 'Process')
    }

    $process = Start-Process -FilePath $exe -WorkingDirectory $sandbox -PassThru
    foreach ($name in $environmentValues.Keys) {
        [Environment]::SetEnvironmentVariable($name, $oldEnvironment[$name], 'Process')
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    $windows = @()
    $mainWindow = $null
    do {
        $process.Refresh()
        if ($process.HasExited) {
            throw "Release process exited before its Aurora window became visible (exit $($process.ExitCode))."
        }

        $windows = @([AuroraReleaseWindowProbe]::GetWindows([uint32]$process.Id))
        $console = @($windows | Where-Object { $_.ClassName -eq 'ConsoleWindowClass' -and $_.Visible })
        if ($console.Count -gt 0) {
            throw 'Release browser created a visible console window.'
        }
        $mainWindow = $windows | Where-Object { $_.Title -eq 'Aurora' -and $_.Visible } | Select-Object -First 1
        if ($null -eq $mainWindow) { Start-Sleep -Milliseconds 250 }
    } while ($null -eq $mainWindow -and [DateTime]::UtcNow -lt $deadline)

    if ($null -eq $mainWindow) {
        $details = ($windows | ForEach-Object { "$($_.ClassName):$($_.Title):visible=$($_.Visible)" }) -join '; '
        throw "Release browser did not show its Aurora window within 30 seconds. Windows: $details"
    }

    if (-not [AuroraReleaseWindowProbe]::RequestClose($mainWindow.Handle)) {
        throw 'Win32 refused the close request for the Aurora window.'
    }
    if (-not $process.WaitForExit(15000)) {
        throw 'Release browser did not exit after WM_CLOSE within 15 seconds.'
    }
    if ($process.ExitCode -ne 0) {
        throw ('Release browser closed with exit code 0x{0:X8}.' -f $process.ExitCode)
    }

    Write-Host 'Release smoke passed: app launched without Rust tools, showed Aurora window, and exited cleanly after WM_CLOSE.'
}
finally {
    foreach ($name in $environmentValues.Keys) {
        [Environment]::SetEnvironmentVariable($name, $oldEnvironment[$name], 'Process')
    }
    if ($null -ne $process) {
        $process.Refresh()
        if (-not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
            $process.WaitForExit(5000) | Out-Null
        }
    }

    $resolvedSandbox = [IO.Path]::GetFullPath($sandbox)
    if (-not $resolvedSandbox.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to remove release smoke data outside TEMP.'
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
            Write-Warning "Could not remove the disposable WebView2 smoke profile; it is retained at $resolvedSandbox"
        }
    }
}
