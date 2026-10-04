<#
.SYNOPSIS
Toggles the default audio playback device between two predefined devices.

.DESCRIPTION
This script checks for the currently active audio playback device. If it's one of the two
target devices, it switches to the other. If it's neither, it defaults to the first device.

This script requires the 'AudioDeviceCmdlets' module, which it will attempt to install
automatically if not found.

.NOTES
You may need to adjust your PowerShell execution policy to run this script.
To do so, open PowerShell as an Administrator and run:
Set-ExecutionPolicy RemoteSigned -Scope CurrentUser
#>

# --- Configuration ---
# Set the names of the two audio devices you want to toggle between.
# These must EXACTLY match the names shown in your Sound settings.
$device1Name = "Digital Audio (S/PDIF) (High Definition Audio Device)"
$device2Name = "PG42UQ (NVIDIA High Definition Audio)"

# --- Script Body ---

# Check if the required module is installed. If not, install it.
if (-not (Get-Module -ListAvailable -Name AudioDeviceCmdlets)) {
    Write-Host "The 'AudioDeviceCmdlets' module is not installed." -ForegroundColor Yellow
    Write-Host "Attempting to install it from the PowerShell Gallery..."
    try {
        Install-Module -Name AudioDeviceCmdlets -Force -Scope CurrentUser -AllowClobber -ErrorAction Stop
        Write-Host "Module installed successfully." -ForegroundColor Green
    } catch {
        Write-Error "Failed to install the 'AudioDeviceCmdlets' module. Please install it manually and try again."
        return
    }
}

# Import the module into the current session
Import-Module AudioDeviceCmdlets

# Get the two target audio device objects
# Using -like with wildcards (*) can help if the name changes slightly, but exact names are more reliable.
$device1 = Get-AudioDevice -List | Where-Object { $_.Name -eq $device1Name }
$device2 = Get-AudioDevice -List | Where-Object { $_.Name -eq $device2Name }

# Error handling if devices are not found
if (-not $device1) {
    Write-Error "Device not found: '$device1Name'. Please check the name in your sound settings."
    return
}
if (-not $device2) {
    Write-Error "Device not found: '$device2Name'. Please check the name in your sound settings."
    return
}

# Get the current default playback device
$currentDevice = Get-AudioDevice -Playback

# Determine which device to switch to and set it
if ($currentDevice.ID -eq $device1.ID) {
    # If the current device is Device 1, switch to Device 2
    Set-AudioDevice -ID $device2.ID
    Write-Host "Switched audio output to: $($device2.Name)" -ForegroundColor Cyan
}
else {
    # Otherwise (if it's Device 2 or any other device), switch to Device 1
    Set-AudioDevice -ID $device1.ID
    Write-Host "Switched audio output to: $($device1.Name)" -ForegroundColor Cyan
}