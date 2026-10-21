$ErrorActionPreference = 'Stop'
python -m pip install -r requirements.txt pyinstaller==6.22.3
python -m PyInstaller --noconfirm --clean --windowed --name Aurora app\main.py
Write-Host 'Build output: dist\Aurora\Aurora.exe'
