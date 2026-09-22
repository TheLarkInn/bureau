[CmdletBinding()]
param([string] $ConfigPath)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:BureauSources = @(
    'supervise.ps1', 'policy.ps1', 'protocol.ps1', 'process.ps1',
    'supervisor.ps1', 'HostNative.cs', 'HostProbe.cs', 'prepare-task.ps1', 'native-budget.json'
)

function Assert-BureauLocalPath {
    param([string] $Path)
    if ($Path -cnotmatch '\A[A-Z]:\\[^\p{Cc}]*\z' -or $Path.Contains('/') -or $Path.Length -gt 240) {
        throw 'noncanonical-local-path'
    }
    if (![string]::Equals([IO.Path]::GetFullPath($Path), $Path, [StringComparison]::Ordinal)) {
        throw 'noncanonical-local-path'
    }
    foreach ($part in $Path.Substring(3).Split('\')) {
        if (!$part -or $part -match '[<>:"|?*\p{Cc}]' -or $part -match '[. ]\z') {
            throw 'noncanonical-local-path'
        }
    }
}

function Assert-BureauRegularPath {
    param([string] $Path)
    Assert-BureauLocalPath $Path
    $current = $Path
    while ($current) {
        if (([IO.File]::GetAttributes($current) -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'reparse-path'
        }
        $current = [IO.Path]::GetDirectoryName($current)
    }
}

function Assert-BureauJsonNames {
    param([System.Text.Json.JsonElement] $Value)
    if ($Value.ValueKind -eq [System.Text.Json.JsonValueKind]::Object) {
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        foreach ($property in $Value.EnumerateObject()) {
            if (!$names.Add($property.Name)) { throw 'duplicate-json-key' }
            Assert-BureauJsonNames $property.Value
        }
    } elseif ($Value.ValueKind -eq [System.Text.Json.JsonValueKind]::Array) {
        foreach ($item in $Value.EnumerateArray()) { Assert-BureauJsonNames $item }
    }
}

function Read-BureauJson {
    param([string] $Text)
    $options = [System.Text.Json.JsonDocumentOptions]::new()
    $options.MaxDepth = 8
    $document = [System.Text.Json.JsonDocument]::Parse($Text, $options)
    try {
        Assert-BureauJsonNames $document.RootElement
        if ($document.RootElement.ValueKind -ne [System.Text.Json.JsonValueKind]::Object) {
            throw 'json-object-required'
        }
        return ConvertFrom-Json -InputObject $Text -AsHashtable -Depth 8
    } finally { $document.Dispose() }
}

function Assert-BureauKeys {
    param($Value, [string[]] $Names)
    if ($Value -isnot [Collections.IDictionary] -or $Value.Count -ne $Names.Count) {
        throw 'invalid-object-keys'
    }
    $expected = [Collections.Generic.HashSet[string]]::new($Names, [StringComparer]::Ordinal)
    foreach ($key in $Value.Keys) {
        if (!$expected.Contains($key)) { throw 'invalid-object-keys' }
    }
}

function Read-BureauLockedFile {
    param([string] $Path, $Locks, [long] $Maximum = 131072)
    Assert-BureauRegularPath $Path
    $stream = [IO.FileStream]::new($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $Locks.Add($stream)
    if ($stream.Length -le 0 -or $stream.Length -gt $Maximum) { throw 'invalid-source-length' }
    $bytes = [byte[]]::new([int] $stream.Length)
    $stream.ReadExactly($bytes)
    return ,$bytes
}

function Read-BureauBundle {
    param([string] $Path, [string] $SourceRoot = $PSScriptRoot)
    $locks = [Collections.Generic.List[IDisposable]]::new()
    try {
        $utf8 = [Text.UTF8Encoding]::new($false, $true)
        $config = Read-BureauJson ($utf8.GetString((Read-BureauLockedFile $Path $locks 65536)))
        if ($config.approved -isnot [bool] -or !$config.approved -or
            $config.qualified -isnot [bool] -or !$config.qualified) { throw 'approval-required' }
        Assert-BureauKeys $config.sourceSha256 $script:BureauSources
        $texts = @{}
        foreach ($name in $script:BureauSources) {
            $bytes = Read-BureauLockedFile ([IO.Path]::Combine($SourceRoot, $name)) $locks
            $pin = $config.sourceSha256[$name]
            $digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
            if ($pin -isnot [string] -or $pin -cnotmatch '\A[0-9a-f]{64}\z' -or $pin -cne $digest) {
                throw 'unreviewed-source'
            }
            $texts[$name] = $utf8.GetString($bytes)
        }
        $nativeBudget = Read-BureauJson $texts['native-budget.json']
        return @{ Config = $config; NativeBudget = $nativeBudget; Texts = $texts; Locks = $locks }
    } catch {
        foreach ($file in $locks) { $file.Dispose() }
        throw 'configuration-or-source-refused'
    }
}

if ($MyInvocation.InvocationName -ne '.') {
    $bundle = $null
    $result = $null
    try {
        if (!$IsWindows -or ![Environment]::Is64BitProcess) { throw 'windows-x64-required' }
        $bundle = Read-BureauBundle $ConfigPath
        foreach ($name in @('policy.ps1', 'protocol.ps1', 'process.ps1', 'supervisor.ps1')) {
            . ([scriptblock]::Create($bundle.Texts[$name]))
        }
        Add-Type -TypeDefinition ($bundle.Texts['HostNative.cs'] + "`n" + $bundle.Texts['HostProbe.cs'])
        $result = Invoke-BureauSupervisor $bundle.Config (New-BureauEffects) $bundle.NativeBudget
    } catch {
        $result = $null
    } finally {
        if ($null -ne $bundle) { foreach ($file in $bundle.Locks) { $file.Dispose() } }
    }
    if ($null -ne $result -and $result.Success) {
        [Console]::WriteLine('Windows supervision completed; native transaction drained.')
        exit 0
    }
    [Console]::Error.WriteLine('Windows supervision refused or failed; no automatic retry.')
    if ($null -ne $result -and $result.Spawned -and !$result.Drained) {
        [Console]::Error.WriteLine('Native drain unconfirmed. Client exit is not proof of native cleanup.')
    }
    exit 1
}
