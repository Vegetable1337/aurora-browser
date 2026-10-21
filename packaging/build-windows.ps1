$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $projectRoot

$python = Join-Path $projectRoot '.venv\Scripts\python.exe'
if (-not (Test-Path -LiteralPath $python)) {
    py -3.13 -m venv (Join-Path $projectRoot '.venv')
    if ($LASTEXITCODE -ne 0) { throw 'Could not create the Python 3.13 virtual environment.' }
}
$pythonVersion = (& $python -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')").Trim()
if ($pythonVersion -ne '3.13') {
    throw ".venv uses Python $pythonVersion. Re-create it with: py -3.13 -m venv .venv"
}

& $python -m pip install -r requirements.txt nuitka==4.3.1
if ($LASTEXITCODE -ne 0) { throw 'Dependency installation failed.' }

$env:VIRTUAL_ENV = Join-Path $projectRoot '.venv'
& $python -m nuitka app\main.py --follow-imports --enable-plugin=pyside6 `
    --output-dir=dist\Nuitka --noinclude-qt-translations --standalone `
    --noinclude-dlls=*.cpp.o --noinclude-dlls=*.qsb `
    --include-qt-plugins=platforminputcontexts --windows-console-mode=disable `
    --assume-yes-for-downloads --output-filename=Aurora.exe
if ($LASTEXITCODE -ne 0) { throw 'Nuitka failed.' }

$executable = Join-Path $projectRoot 'dist\Nuitka\main.dist\Aurora.exe'
if (-not (Test-Path -LiteralPath $executable)) { throw 'Build did not produce dist\Nuitka\main.dist\Aurora.exe.' }
$process = Start-Process -FilePath $executable -ArgumentList '--smoke-test' -WindowStyle Hidden -PassThru
if (-not $process.WaitForExit(30000)) {
    Stop-Process -Id $process.Id -Force
    throw 'Aurora did not finish its packaged browser smoke test within 30 seconds.'
}
if ($process.ExitCode -ne 0) { throw "Packaged browser smoke test failed with exit code $($process.ExitCode)." }
Write-Host "Build output: $executable"
Write-Host 'Packaged browser smoke test passed. Launch Aurora.exe to run the portable Windows build.'
