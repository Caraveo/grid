<#
  GRID host-runtime preflight for Windows.

  GRID host containers are Linux containerd workloads. The supported Windows
  workflow is WSL2: install and run GRID inside an Ubuntu/Debian WSL2 distro,
  where the normal Linux installer verifies `nerdctl info`. Native Windows
  containers are deliberately not used because they cannot enforce the same
  Linux namespace/capability isolation contract.
#>
param([string]$Distro = "Ubuntu")

$ErrorActionPreference = "Stop"

if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
  Write-Error "WSL2 is required. In an elevated PowerShell run: wsl --install -d $Distro; then reboot."
}

$listed = wsl.exe -l -q 2>$null
if ($listed -notcontains $Distro) {
  Write-Host "Install the GRID Linux runtime with: wsl --install -d $Distro"
  exit 1
}

$ready = wsl.exe -d $Distro -- sh -lc 'command -v nerdctl >/dev/null && nerdctl info >/dev/null' 2>$null
if ($LASTEXITCODE -ne 0) {
  Write-Host "WSL distro '$Distro' has no ready rootless containerd/nerdctl runtime."
  Write-Host "Open it with: wsl -d $Distro"
  Write-Host "Then install the Linux containerd + nerdctl runtime and run: nerdctl info"
  exit 1
}

Write-Host "GRID host runtime ready in WSL2 '$Distro'. Run GRID inside WSL: wsl -d $Distro"
