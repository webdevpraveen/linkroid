# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
    Registers the Linkroid Credential Provider COM DLL with Windows LogonUI.
.DESCRIPTION
    Requires Administrator privileges.
    Writes the COM InprocServer32 entry and adds the CLSID under
    HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers.
.PARAMETER DllPath
    Optional path to linkroid_credential_provider.dll. Defaults to the built target path.
#>
[CmdletBinding()]
param(
    [string]$DllPath
)

$ErrorActionPreference = "Stop"

# Ensure running as Administrator
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Error "This script must be run as Administrator in PowerShell."
    exit 1
}

$Clsid = "{7C49E211-B4D8-4F92-B981-6B4A3A5F82C1}"

if (-not $DllPath) {
    $scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
    $repoRoot = Split-Path -Parent $scriptRoot
    $candidate1 = Join-Path $repoRoot "target\release\linkroid_credential_provider.dll"
    $candidate2 = Join-Path $repoRoot "target\debug\linkroid_credential_provider.dll"
    if (Test-Path $candidate1) {
        $DllPath = (Resolve-Path $candidate1).Path
    } elseif (Test-Path $candidate2) {
        $DllPath = (Resolve-Path $candidate2).Path
    } else {
        $DllPath = $candidate1
    }
}

if (-not (Test-Path $DllPath)) {
    Write-Warning "DLL not found at: $DllPath"
    Write-Warning "Build it first with: cargo build --package linkroid-credential-provider --release"
}

Write-Host "Registering Linkroid Credential Provider..." -ForegroundColor Cyan
Write-Host "  CLSID: $Clsid"
Write-Host "  DLL:   $DllPath"

# 1. Register CLSID under HKCR
$clsidPath = "Registry::HKEY_CLASSES_ROOT\CLSID\$Clsid"
$inprocPath = "$clsidPath\InprocServer32"

New-Item -Path $clsidPath -Force | Out-Null
Set-ItemProperty -Path $clsidPath -Name "(Default)" -Value "Linkroid Credential Provider"

New-Item -Path $inprocPath -Force | Out-Null
Set-ItemProperty -Path $inprocPath -Name "(Default)" -Value $DllPath
Set-ItemProperty -Path $inprocPath -Name "ThreadingModel" -Value "Apartment"

# 2. Register with LogonUI Credential Providers
$cpPath = "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$Clsid"
New-Item -Path $cpPath -Force | Out-Null
Set-ItemProperty -Path $cpPath -Name "(Default)" -Value "LinkroidCredentialProvider"

Write-Host "Successfully registered Linkroid Credential Provider." -ForegroundColor Green
Write-Host ""
Write-Host "SAFETY & RECOVERY INSTRUCTIONS:" -ForegroundColor Yellow
Write-Host "  - Windows default password tile remains fully active and is never replaced."
Write-Host "  - To unregister at any time, run: .\scripts\unregister-credential-provider.ps1"
Write-Host "  - Emergency recovery: boot into Safe Mode and delete $cpPath"
