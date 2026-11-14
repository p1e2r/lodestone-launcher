# Lodestone installer for Windows.
#
#   iex ((New-Object Net.WebClient).DownloadString('https://p1e2r.github.io/lodestone-launcher/install.ps1'))
#
# Downloads the latest installer and installs Lodestone for the current user.
# No admin rights needed. Your instances and worlds are kept.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # makes Invoke-WebRequest much faster

function Say($msg) { Write-Host "==> $msg" -ForegroundColor Green }

if (-not [Environment]::Is64BitOperatingSystem) {
    throw "Lodestone needs 64-bit Windows."
}

[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$url = 'https://github.com/p1e2r/lodestone-launcher/releases/latest/download/Lodestone-windows-x64-setup.exe'
$setup = Join-Path $env:TEMP 'Lodestone-windows-x64-setup.exe'

Say 'Downloading Lodestone for Windows'
Invoke-WebRequest -Uri $url -OutFile $setup -UseBasicParsing

$running = Get-Process -Name 'Lodestone' -ErrorAction SilentlyContinue
if ($running) {
    Say 'Closing the running Lodestone'
    $running | Stop-Process -Force
    Start-Sleep -Seconds 2
}

Say 'Installing'
$p = Start-Process -FilePath $setup -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait -PassThru
Remove-Item $setup -Force -ErrorAction SilentlyContinue
if ($p.ExitCode -ne 0) { throw "The installer failed (exit code $($p.ExitCode))." }

$exe = Join-Path $env:LOCALAPPDATA 'Programs\Lodestone\Lodestone.exe'
if (-not (Test-Path $exe)) { throw "Lodestone wasn't found at $exe after installing." }

Say 'Done! Opening Lodestone'
Start-Process -FilePath $exe
Write-Host 'Find it later in the Start Menu.'
