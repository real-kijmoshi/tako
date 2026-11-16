# Install Tako Code on Windows (PowerShell):
#   irm https://github.com/real-kijmoshi/tako/releases/latest/download/install-code.ps1 | iex
$ErrorActionPreference = 'Stop'
$repo = if ($env:TAKO_REPO) { $env:TAKO_REPO } else { 'real-kijmoshi/tako' }
$dir = if ($env:TAKO_INSTALL_DIR) { $env:TAKO_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\tako' }
$version = if ($env:TAKO_VERSION) { $env:TAKO_VERSION } else { 'latest' }
if (-not [Environment]::Is64BitOperatingSystem) { throw 'Tako Code needs 64-bit Windows.' }
$asset = 'tako-code-windows-x86_64.exe'
if ($version -eq 'latest') { $base = "https://github.com/$repo/releases/latest/download" }
elseif ($version -match '^v[0-9]') { $base = "https://github.com/$repo/releases/download/$version" }
else { throw 'TAKO_VERSION must be latest or a tag such as v0.1.9' }

New-Item -ItemType Directory -Force -Path $dir | Out-Null
$work = Join-Path $dir ('.tako-code-install-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
try {
  Write-Host 'Downloading Tako Code for windows/x86_64...'
  Invoke-WebRequest "$base/$asset" -OutFile (Join-Path $work $asset) -UseBasicParsing
  Invoke-WebRequest "$base/SHA256SUMS" -OutFile (Join-Path $work 'SHA256SUMS') -UseBasicParsing
  $lines = @(Get-Content (Join-Path $work 'SHA256SUMS') | Where-Object { ($_ -split '\s+')[1] -eq $asset })
  if ($lines.Count -ne 1) { throw 'Release checksum missing or ambiguous. Existing installation kept.' }
  $expected = ($lines[0] -split '\s+')[0].ToLower()
  $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $work $asset)).Hash.ToLower()
  if ($expected -ne $actual) { throw 'Checksum failed. Existing installation kept.' }
  Move-Item -Force (Join-Path $work $asset) (Join-Path $dir 'tako-code.exe')
} finally {
  Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not (($userPath -split ';') -contains $dir)) {
  [Environment]::SetEnvironmentVariable('Path', ($userPath.TrimEnd(';') + ';' + $dir), 'User')
  $env:Path += ';' + $dir
  Write-Host "Added $dir to your user PATH (open a new terminal to use it everywhere)."
}
Write-Host ("Installed " + (Join-Path $dir 'tako-code.exe') + " (" + (& (Join-Path $dir 'tako-code.exe') version) + ")")
if (-not (Get-Command git -ErrorAction SilentlyContinue)) { Write-Host 'Note: Tako Code needs Git. Install it before your first swarm.' }
Write-Host "`nNext: cd into a Git repository and run  tako-code"
