function Assert-BureauInteger {
    param($Value, [long] $Minimum, [long] $Maximum)
    if (($Value -isnot [int] -and $Value -isnot [long] -and $Value -isnot [uint32] -and
         $Value -isnot [uint64]) -or $Value -lt $Minimum -or $Value -gt $Maximum) {
        throw 'invalid-integer'
    }
}

function Assert-BureauConfig {
    param($Config)
    Assert-BureauKeys $Config @('schema', 'approved', 'qualified', 'windowsUserSid', 'registrationId',
        'distro', 'commit', 'vhdPath', 'vhdIdentity', 'limits', 'sourceSha256')
    if ($Config.schema -isnot [string] -or $Config.schema -cnotmatch '\Abureau-windows-supervision-v1\z' -or
        $Config.approved -isnot [bool] -or !$Config.approved -or
        $Config.qualified -isnot [bool] -or !$Config.qualified) { throw 'approval-required' }
    foreach ($field in @('windowsUserSid', 'registrationId', 'distro', 'commit', 'vhdPath')) {
        if ($Config[$field] -isnot [string]) { throw 'invalid-configuration-string' }
    }
    if ($Config.windowsUserSid -cnotmatch '\AS-1-(5-21|12-1)-([0-9]+-){3}[0-9]+\z' -or
        $Config.registrationId -cnotmatch '\A\{[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}\}\z' -or
        $Config.distro -cnotmatch '\A[A-Za-z0-9][A-Za-z0-9._-]{0,63}\z' -or
        $Config.commit -cnotmatch '\A[0-9a-f]{40}\z' -or
        $Config.commit -ceq ('0' * 40)) { throw 'invalid-configuration-identity' }
    if ([Security.Principal.SecurityIdentifier]::new($Config.windowsUserSid).Value -cne $Config.windowsUserSid) {
        throw 'noncanonical-user-sid'
    }
    Assert-BureauLocalPath $Config.vhdPath
    Assert-BureauVhdPins $Config.vhdIdentity
    Assert-BureauLimits $Config.limits
}

function Assert-BureauVhdPins {
    param($Pins)
    Assert-BureauKeys $Pins @('volumePath', 'volumeSerial', 'fileId')
    if ($Pins.volumePath -isnot [string] -or $Pins.volumeSerial -isnot [string] -or
        $Pins.fileId -isnot [string]) { throw 'invalid-vhd-pins' }
    if ($Pins.volumePath -cnotmatch '\A\\\\\?\\Volume\{[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}\}\\\z' -or
        $Pins.volumeSerial -cnotmatch '\A[0-9a-f]{16}\z' -or
        $Pins.fileId -cnotmatch '\A[0-9a-f]{32}\z' -or $Pins.fileId -eq ('0' * 32)) {
        throw 'invalid-vhd-pins'
    }
}

function Assert-BureauLimits {
    param($Limits)
    Assert-BureauKeys $Limits @('vhdBytesMax', 'backingFreeBytesMin', 'memoryFreeBytesMin')
    Assert-BureauInteger $Limits.vhdBytesMax 1 15032385536
    Assert-BureauInteger $Limits.backingFreeBytesMin 8589934592 ([long]::MaxValue)
    Assert-BureauInteger $Limits.memoryFreeBytesMin 1073741824 ([long]::MaxValue)
}

function Assert-BureauRegistration {
    param($Sample, $Config)
    Assert-BureauInteger $Sample.Version 2 2
    if ($Sample.WindowsUserSid -cne $Config.windowsUserSid -or $Sample.Distro -cne $Config.distro -or
        $Sample.RegistrationId -cne $Config.registrationId -or $Sample.Version -ne 2) {
        throw 'registration-mismatch'
    }
    Assert-BureauLocalPath $Sample.BasePath
    if ($Sample.VhdFileName -isnot [string] -or
        $Sample.VhdFileName -cnotmatch '\A[^\\/:*?"<>|\p{Cc}]+(?<![. ])\z' -or
        $Sample.RegistrationStamp -isnot [string] -or
        $Sample.RegistrationStamp -cnotmatch '\A[0-9a-f]{16}\z' -or
        $Sample.RegistrationStamp -ceq ('0' * 16)) { throw 'registration-unmeasurable' }
    $expected = [IO.Path]::Combine($Sample.BasePath, $Sample.VhdFileName)
    if (![string]::Equals($expected, $Config.vhdPath, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'registration-vhd-mismatch'
    }
}

function Assert-BureauSample {
    param($Sample, $Config, [long] $Started, $Previous)
    $now = [Diagnostics.Stopwatch]::GetTimestamp()
    Assert-BureauInteger $Sample.StartedTicks $Started $now
    Assert-BureauInteger $Sample.FinishedTicks $Sample.StartedTicks $now
    if (($now - $Started) * 1000.0 / [Diagnostics.Stopwatch]::Frequency -gt 2000) { throw 'stale-sample' }
    Assert-BureauRegistration $Sample $Config
    Assert-BureauFileSample $Sample $Config
    Assert-BureauSampleLimits $Sample $Config.limits
    if ($null -ne $Previous) { Assert-BureauSameIdentity $Sample $Previous }
}

function Assert-BureauFileSample {
    param($Sample, $Config)
    Assert-BureauInteger $Sample.Attributes 1 ([uint32]::MaxValue)
    Assert-BureauInteger $Sample.Links 1 1
    Assert-BureauInteger $Sample.FileType 1 1
    Assert-BureauInteger $Sample.DriveType 3 3
    if ($Sample.DeletePending -isnot [bool]) { throw 'unmeasurable-vhd-file' }
    if ($Sample.Attributes -band 0x410 -or $Sample.Links -ne 1 -or $Sample.DeletePending -or
        $Sample.FileType -ne 1 -or $Sample.DriveType -ne 3) { throw 'ambiguous-vhd-file' }
    if (![string]::Equals($Sample.FinalPath, $Config.vhdPath, [StringComparison]::OrdinalIgnoreCase) -or
        $Sample.VolumePath -cne $Config.vhdIdentity.volumePath -or
        $Sample.VolumeSerial -cne $Config.vhdIdentity.volumeSerial -or
        $Sample.FileId -cne $Config.vhdIdentity.fileId) { throw 'vhd-identity-mismatch' }
}

function Assert-BureauSampleLimits {
    param($Sample, $Limits)
    foreach ($field in @('PhysicalBytes', 'LogicalBytes', 'BackingFreeBytes', 'MemoryFreeBytes')) {
        Assert-BureauInteger $Sample.$field 1 ([long]::MaxValue)
    }
    if ($Sample.PhysicalBytes -ge $Limits.vhdBytesMax -or $Sample.LogicalBytes -ge $Limits.vhdBytesMax -or
        $Sample.BackingFreeBytes -lt $Limits.backingFreeBytesMin -or
        $Sample.MemoryFreeBytes -lt $Limits.memoryFreeBytesMin) { throw 'host-resource-limit' }
}

function Assert-BureauStartupReserve {
    param($Sample, $Limits)
    $effective = [Math]::Max([long] $Sample.PhysicalBytes, [long] $Sample.LogicalBytes)
    $ceiling = $Limits.vhdBytesMax - 1073741824
    if ($effective -gt $ceiling) { throw 'startup-vhd-reserve' }
}

function Assert-BureauNativeBudget {
    param($Budget)
    Assert-BureauKeys $Budget @('schema', 'engineMemoryMaxBytes', 'guardianMemoryMaxBytes')
    if ($Budget.schema -isnot [string] -or $Budget.schema -cnotmatch '\Abureau-native-budget-v1\z') {
        throw 'native-budget-schema'
    }
    foreach ($field in @('engineMemoryMaxBytes', 'guardianMemoryMaxBytes')) {
        Assert-BureauInteger $Budget[$field] 1 ([long]::MaxValue)
    }
}

function Get-BureauStartupMemoryMinimum {
    param($Budget, $Limits)
    $required = [decimal] $Budget.engineMemoryMaxBytes + [decimal] $Budget.guardianMemoryMaxBytes +
        [decimal] $Limits.memoryFreeBytesMin
    if ($required -gt [long]::MaxValue) { throw 'native-memory-budget-overflow' }
    return [long] $required
}

function Assert-BureauStartupMemory {
    param($Sample, $Budget, $Limits)
    $minimum = Get-BureauStartupMemoryMinimum $Budget $Limits
    if ($Sample.MemoryFreeBytes -lt $minimum) { throw 'startup-memory-reserve' }
}

function Assert-BureauSameIdentity {
    param($Current, $Previous)
    foreach ($field in @('WindowsUserSid', 'Distro', 'RegistrationId', 'RegistrationStamp', 'Version',
        'BasePath', 'VhdFileName', 'FinalPath', 'VolumePath', 'VolumeSerial', 'FileId')) {
        if ($Current.$field -cne $Previous.$field) { throw 'host-identity-changed' }
    }
}
