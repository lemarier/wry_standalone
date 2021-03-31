$ErrorActionPreference = 'Stop'

if ($v) {
  $Version = "v${v}"
}
if ($args.Length -eq 1) {
  $Version = $args.Get(0)
}

$WRYInstall = $env:DENO_INSTALL
$BinDir = if ($WRYInstall) {
  "$WRYInstall\bin"
} else {
  "$Home\.wry\bin"
}

$WRYZip = "$BinDir\wry.zip"
$WRYExe = "$BinDir\wry.exe"
$Target = 'x86_64-pc-windows-msvc'

# GitHub requires TLS 1.2
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$WRYUri = if (!$Version) {
  "https://github.com/lemarier/wry_standalone/releases/latest/download/wry-${Target}.zip"
} else {
  "https://github.com/lemarier/wry_standalone/releases/download/${Version}/wry-${Target}.zip"
}

if (!(Test-Path $BinDir)) {
  New-Item $BinDir -ItemType Directory | Out-Null
}

Invoke-WebRequest $WRYUri -OutFile $WRYZip -UseBasicParsing

if (Get-Command Expand-Archive -ErrorAction SilentlyContinue) {
  Expand-Archive $WRYZip -Destination $BinDir -Force
} else {
  if (Test-Path $WRYExe) {
    Remove-Item $WRYExe
  }
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  [IO.Compression.ZipFile]::ExtractToDirectory($WRYZip, $BinDir)
}

Remove-Item $WRYZip

$User = [EnvironmentVariableTarget]::User
$Path = [Environment]::GetEnvironmentVariable('Path', $User)
if (!(";$Path;".ToLower() -like "*;$BinDir;*".ToLower())) {
  [Environment]::SetEnvironmentVariable('Path', "$Path;$BinDir", $User)
  $Env:Path += ";$BinDir"
}

Write-Output "wry was installed successfully to $WRYExe"
Write-Output "Run 'wry --help' to get started"