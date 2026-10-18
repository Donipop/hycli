param(
    [string]$Prefix = ""
)
$ErrorActionPreference = "Stop"
$SourceDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$DefaultInstall = Join-Path $env:LOCALAPPDATA "Programs\Hycli"
$InstallDir = if ([string]::IsNullOrWhiteSpace($Prefix)) { $DefaultInstall } else { [System.IO.Path]::GetFullPath($Prefix) }

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$ExistingCli = Join-Path $InstallDir "hycli.exe"
if (Test-Path $ExistingCli) {
    & $ExistingCli stop | Out-Null
}

foreach ($File in @("hycli.exe", "hycli-desktop.exe", "LICENSE", "README.md", "hycli.ico")) {
    $Src = Join-Path $SourceDir $File
    if (Test-Path $Src) {
        Copy-Item -Path $Src -Destination (Join-Path $InstallDir $File) -Force
    }
}

if ([string]::IsNullOrWhiteSpace($Prefix)) {
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $UserPath) { $UserPath = "" }
    $Parts = $UserPath -split ";" | Where-Object { $_ -ne "" }
    if ($Parts -notcontains $InstallDir) {
        $NewPath = ($Parts + $InstallDir) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    }
    $env:Path = "$InstallDir;$env:Path"

    $WshShell = New-Object -ComObject WScript.Shell
    $StartMenuDir = Join-Path ([Environment]::GetFolderPath("Programs")) "Hycli"
    New-Item -ItemType Directory -Force -Path $StartMenuDir | Out-Null

    $LaunchShortcut = $WshShell.CreateShortcut((Join-Path $StartMenuDir "Hycli.lnk"))
    $LaunchShortcut.TargetPath = (Join-Path $InstallDir "hycli-desktop.exe")
    $LaunchShortcut.WorkingDirectory = $InstallDir
    if (Test-Path (Join-Path $InstallDir "hycli.ico")) {
        $LaunchShortcut.IconLocation = (Join-Path $InstallDir "hycli.ico")
    }
    $LaunchShortcut.Save()

    $QuitShortcut = $WshShell.CreateShortcut((Join-Path $StartMenuDir "Quit Hycli.lnk"))
    $QuitShortcut.TargetPath = (Join-Path $InstallDir "hycli-desktop.exe")
    $QuitShortcut.Arguments = "stop"
    $QuitShortcut.WorkingDirectory = $InstallDir
    if (Test-Path (Join-Path $InstallDir "hycli.ico")) {
        $QuitShortcut.IconLocation = (Join-Path $InstallDir "hycli.ico")
    }
    $QuitShortcut.Save()
}

Write-Output "Installed Hycli to $InstallDir. Run 'hycli open' to start."
