# Unpacks the Windows archive of the GitHub release into the package folder; Chocolatey puts a
# shim for rudo.exe on the PATH and removes both when the package is uninstalled.
$ErrorActionPreference = 'Stop'

$url64 = 'https://github.com/Altrabeth/RuDo/releases/download/v1.0.0/rudo-tui-x86_64-pc-windows-msvc.zip'

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = Split-Path -Parent $MyInvocation.MyCommand.Definition
  url64bit       = $url64
  checksum64     = '0000000000000000000000000000000000000000000000000000000000000000'
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
