# Rusty installer — Windows (PowerShell).
#
#   irm https://raw.githubusercontent.com/codingsushi79/rusty/main/scripts/install.ps1 | iex
#
# Builds Rusty and installs the `rusty` command to %LOCALAPPDATA%\Programs\Rusty,
# so you can run `rusty <dir>` in any terminal.
$ErrorActionPreference = 'Stop'
$Repo   = if ($env:RUSTY_REPO)   { $env:RUSTY_REPO }   else { 'https://github.com/codingsushi79/rusty' }
$Branch = if ($env:RUSTY_BRANCH) { $env:RUSTY_BRANCH } else { 'main' }
$Dest   = Join-Path $env:LOCALAPPDATA 'Programs\Rusty'
function Say($m) { Write-Host "▸ $m" -ForegroundColor DarkYellow }

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  Say 'Rust not found — installing via rustup…'
  $ru = "$env:TEMP\rustup-init.exe"
  Invoke-WebRequest -Uri 'https://win.rustup.rs/x86_64' -OutFile $ru
  & $ru -y | Out-Null
  $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
}

$Work = Join-Path $env:TEMP ("rusty-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Work | Out-Null
try {
  Say 'Fetching source…'
  git clone --depth 1 --branch $Branch $Repo "$Work\rusty" | Out-Null
  Say 'Building (first build takes a few minutes)…'
  Push-Location "$Work\rusty"; cargo build --release; Pop-Location

  New-Item -ItemType Directory -Force -Path $Dest | Out-Null
  Copy-Item "$Work\rusty\target\release\rusty.exe" "$Dest\rusty.exe" -Force
  Say "Installed to $Dest\rusty.exe"

  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  if ($userPath -notlike "*$Dest*") {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$Dest", 'User')
    Say 'Added Rusty to your PATH (restart your terminal).'
  }
  Say 'Done. Run:  rusty .'
}
finally {
  Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}
