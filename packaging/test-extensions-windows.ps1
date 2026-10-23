$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$binary = Join-Path $projectRoot 'rust-browser/target/debug/examples/extensions_probe.exe'
$fixture = Join-Path $projectRoot 'rust-browser/fixtures/mv3-probe'
$runRoot = Join-Path $projectRoot ('rust-browser/target/extensions-check-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $runRoot | Out-Null
Set-Content (Join-Path $runRoot '.aurora-extension-probe') 'Disposable extension experiment data; not an Aurora user profile.'
$profileA = Join-Path $runRoot 'A'
$profileB = Join-Path $runRoot 'B'
$results = [System.Collections.Generic.List[object]]::new()
function Check-Probe($operation, $profile, $expected, $expectedCode = 0) {
    # Each invocation is a new OS process: persistence is tested across restarts.
    $stdout = Join-Path $runRoot ($results.Count.ToString() + '.out')
    $stderr = Join-Path $runRoot ($results.Count.ToString() + '.err')
    $process = Start-Process -FilePath $binary -ArgumentList @($operation, ('"'+$profile+'"'), ('"'+$fixture+'"')) -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    if (-not $process.WaitForExit(30000)) { Stop-Process -Id $process.Id; throw 'Probe process exceeded 30 seconds' }
    $output = (Get-Content $stdout -Raw) + (Get-Content $stderr -Raw)
    $passed = $process.ExitCode -eq $expectedCode -and $output.Contains($expected)
    $results.Add(@{operation=$operation;profile=(Split-Path $profile -Leaf);exit=$process.ExitCode;passed=$passed;output=$output})
    $results | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $runRoot 'results.json')
    if (-not $passed) { throw "Failed $operation (exit $($process.ExitCode)): $output" }
    Write-Host "PASS $operation $(Split-Path $profile -Leaf)"
}
Write-Host "Isolated test data: $runRoot"
Check-Probe 'install' $profileA 'installed'
Check-Probe 'list' $profileA 'fixture_count=1 enabled=true'
Check-Probe 'probe' $profileA '"visits":1'
Check-Probe 'probe' $profileA '"visits":2'
Check-Probe 'list' $profileB 'fixture_count=0'
Check-Probe 'install' $profileB 'installed'
Check-Probe 'probe' $profileB '"visits":1'
Check-Probe 'disable' $profileA 'enabled=false'
Check-Probe 'list' $profileA 'fixture_count=1 enabled=false'
Check-Probe 'probe' $profileB '"visits":2'
Check-Probe 'probe' $profileA '20s timeout' 1
Check-Probe 'enable' $profileA 'enabled=true'
Check-Probe 'probe' $profileA '"visits":3'
Check-Probe 'remove' $profileA 'removed'
Check-Probe 'list' $profileA 'fixture_count=0'
Check-Probe 'list' $profileB 'fixture_count=1 enabled=true'
Write-Host "All extension lifecycle checks passed; results saved in $runRoot"
