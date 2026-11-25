# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
    Unregisters the Linkroid Credential Provider COM DLL from Windows LogonUI.
.DESCRIPTION
    Requires Administrator privileges.
    Removes the CLSID from LogonUI and deletes the COM InprocServer32 registration.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Error "This script must be run as Administrator in PowerShell."
    exit 1
}

$Clsid = "{7C49E211-B4D8-4F92-B981-6B4A3A5F82C1}"

Write-Host "Unregistering Linkroid Credential Provider..." -ForegroundColor Cyan

# 1. Remove from LogonUI Credential Providers
$cpPath = "Registry::HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$Clsid"
if (Test-Path $cpPath) {
    Remove-Item -Path $cpPath -Recurse -Force
    Write-Host "  Removed LogonUI registration: $cpPath"
} else {
    Write-Host "  LogonUI key not found (already unregistered)."
}

# 2. Remove from HKCR\CLSID
$clsidPath = "Registry::HKEY_CLASSES_ROOT\CLSID\$Clsid"
if (Test-Path $clsidPath) {
    Remove-Item -Path $clsidPath -Recurse -Force
    Write-Host "  Removed COM registration: $clsidPath"
} else {
    Write-Host "  COM key not found (already unregistered)."
}

Write-Host "Successfully unregistered Linkroid Credential Provider." -ForegroundColor Green
