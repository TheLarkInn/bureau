[CmdletBinding()]
param(
    [string] $UserSid, [string] $PwshPath, [string] $PwshSha256,
    [string] $SupervisorPath, [string] $SupervisorSha256,
    [string] $ConfigPath, [string] $ConfigSha256, [string] $OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-BureauTaskPath {
    param([string] $Path)
    if ($Path -cnotmatch '\A[A-Z]:\\[^\p{Cc}]*\z' -or $Path.Contains('/') -or
        ![string]::Equals([IO.Path]::GetFullPath($Path), $Path, [StringComparison]::Ordinal) -or
        $Path.Length -gt 240) { throw 'invalid-task-path' }
    foreach ($part in $Path.Substring(3).Split('\')) {
        if (!$part -or $part -match '[<>:"|?*\p{Cc}]|[. ]\z') { throw 'invalid-task-path' }
    }
    $parent = [IO.Path]::GetDirectoryName($Path)
    while ($parent) {
        if ([IO.File]::GetAttributes($parent) -band [IO.FileAttributes]::ReparsePoint) { throw 'task-reparse-path' }
        $parent = [IO.Path]::GetDirectoryName($parent)
    }
}

function Read-BureauTaskPin {
    param([string] $Path, [string] $Digest, $Locks)
    Assert-BureauTaskPath $Path
    if ([IO.File]::GetAttributes($Path) -band [IO.FileAttributes]::ReparsePoint) { throw 'task-reparse-file' }
    if ($Digest -cnotmatch '\A[0-9a-f]{64}\z') { throw 'invalid-task-digest' }
    $file = [IO.FileStream]::new($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $Locks.Add($file)
    $actual = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($file)).ToLowerInvariant()
    if ($actual -cne $Digest) { throw 'task-digest-mismatch' }
}

function Assert-BureauTaskUser {
    param([string] $Path, [string] $Sid)
    $document = [Text.Json.JsonDocument]::Parse([IO.File]::ReadAllText($Path))
    try {
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        foreach ($property in $document.RootElement.EnumerateObject()) {
            if (!$names.Add($property.Name)) { throw 'duplicate-task-config-key' }
        }
        if (![string]::Equals($document.RootElement.GetProperty('windowsUserSid').GetString(),
            $Sid, [StringComparison]::Ordinal)) {
            throw 'task-user-config-mismatch'
        }
    } finally { $document.Dispose() }
}

function Write-BureauTaskElement {
    param($Writer, [string] $Name, [string] $Value)
    $Writer.WriteElementString($Name, 'http://schemas.microsoft.com/windows/2004/02/mit/task', $Value)
}

function New-BureauLogonTaskXml {
    param([string] $Sid, [string] $Pwsh, [string] $Supervisor, [string] $Config)
    if ($Sid -cnotmatch '\AS-1-(5-21|12-1)-([0-9]+-){3}[0-9]+\z' -or
        [Security.Principal.SecurityIdentifier]::new($Sid).Value -cne $Sid) { throw 'explicit-user-sid-required' }
    foreach ($path in @($Pwsh, $Supervisor, $Config)) { Assert-BureauTaskPath $path }
    $text = [Text.StringBuilder]::new()
    $settings = [Xml.XmlWriterSettings]::new()
    $settings.Indent = $true
    $settings.OmitXmlDeclaration = $true
    $writer = [Xml.XmlWriter]::Create($text, $settings)
    try {
        $writer.WriteStartElement('Task', 'http://schemas.microsoft.com/windows/2004/02/mit/task')
        $writer.WriteAttributeString('version', '1.4')
        Write-BureauTaskTrigger $writer $Sid
        Write-BureauTaskPrincipal $writer $Sid
        Write-BureauTaskSettings $writer
        Write-BureauTaskAction $writer $Pwsh $Supervisor $Config
        $writer.WriteEndElement()
        $writer.Flush()
        return $text.ToString()
    } finally { $writer.Dispose() }
}

function Write-BureauTaskTrigger {
    param($Writer, [string] $Sid)
    $Writer.WriteStartElement('RegistrationInfo')
    Write-BureauTaskElement $Writer 'Description' 'Disabled review-only user-logon preparation. Not pre-login boot or logout survival. Qualification and installation remain explicit operator work.'
    $Writer.WriteEndElement()
    $Writer.WriteStartElement('Triggers')
    $Writer.WriteStartElement('LogonTrigger')
    Write-BureauTaskElement $Writer 'Enabled' 'false'
    Write-BureauTaskElement $Writer 'UserId' $Sid
    $Writer.WriteEndElement()
    $Writer.WriteEndElement()
}

function Write-BureauTaskPrincipal {
    param($Writer, [string] $Sid)
    $Writer.WriteStartElement('Principals')
    $Writer.WriteStartElement('Principal')
    $Writer.WriteAttributeString('id', 'selected-user')
    Write-BureauTaskElement $Writer 'UserId' $Sid
    Write-BureauTaskElement $Writer 'LogonType' 'InteractiveToken'
    Write-BureauTaskElement $Writer 'RunLevel' 'LeastPrivilege'
    $Writer.WriteEndElement()
    $Writer.WriteEndElement()
}

function Write-BureauTaskSettings {
    param($Writer)
    $Writer.WriteStartElement('Settings')
    foreach ($pair in @(@('MultipleInstancesPolicy', 'IgnoreNew'), @('DisallowStartIfOnBatteries', 'false'),
        @('StopIfGoingOnBatteries', 'false'), @('StartWhenAvailable', 'false'),
        @('AllowStartOnDemand', 'false'), @('Enabled', 'false'), @('ExecutionTimeLimit', 'PT0S'))) {
        Write-BureauTaskElement $Writer $pair[0] $pair[1]
    }
    $Writer.WriteEndElement()
}

function Write-BureauTaskAction {
    param($Writer, [string] $Pwsh, [string] $Supervisor, [string] $Config)
    $Writer.WriteStartElement('Actions')
    $Writer.WriteAttributeString('Context', 'selected-user')
    $Writer.WriteStartElement('Exec')
    Write-BureauTaskElement $Writer 'Command' $Pwsh
    Write-BureauTaskElement $Writer 'Arguments' "-NoProfile -NonInteractive -File `"$Supervisor`" -ConfigPath `"$Config`""
    Write-BureauTaskElement $Writer 'WorkingDirectory' ([IO.Path]::GetDirectoryName($Supervisor))
    $Writer.WriteEndElement()
    $Writer.WriteEndElement()
}

if ($MyInvocation.InvocationName -ne '.') {
    $locks = [Collections.Generic.List[IDisposable]]::new()
    try {
        if ([IO.Path]::GetFileName($PwshPath) -cne 'pwsh.exe') { throw 'explicit-pwsh-required' }
        Read-BureauTaskPin $PwshPath $PwshSha256 $locks
        Read-BureauTaskPin $SupervisorPath $SupervisorSha256 $locks
        Read-BureauTaskPin $ConfigPath $ConfigSha256 $locks
        Assert-BureauTaskUser $ConfigPath $UserSid
        $xml = New-BureauLogonTaskXml $UserSid $PwshPath $SupervisorPath $ConfigPath
        if (!$OutputPath) { [Console]::WriteLine($xml) } else {
            Assert-BureauTaskPath $OutputPath
            $output = [IO.FileStream]::new($OutputPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
            try { $output.Write([Text.UTF8Encoding]::new($false).GetBytes($xml)); $output.Flush($true) }
            finally { $output.Dispose() }
        }
    } catch {
        [Console]::Error.WriteLine('Disabled task preparation refused. Nothing was registered or started.')
        exit 1
    } finally { foreach ($file in $locks) { $file.Dispose() } }
}
