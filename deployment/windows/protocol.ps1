function New-BureauProtocol {
    return @{ Sequence = $null; Guard = $null; Identity = $null; Running = $false;
        Stopped = $false; LastChallengeTicks = [long] 0 }
}

function Read-BureauFrame {
    param([string] $Text, $State)
    $frame = Read-BureauJson $Text
    if ($State.Stopped -or $frame.schema -isnot [string] -or $frame.schema -cnotmatch '\Abureau-windows-v1\z' -or
        $frame.type -isnot [string]) { throw 'invalid-native-frame' }
    if ($frame.type -cmatch '\Astopped\z') {
        Assert-BureauKeys $frame @('schema', 'type', 'drained')
        if ($frame.drained -isnot [bool] -or !$frame.drained) {
            throw 'invalid-native-drain'
        }
        $State.Stopped = $true
        return $frame
    }
    Assert-BureauChallenge $frame $State
    return $frame
}

function Assert-BureauChallenge {
    param($Frame, $State)
    Assert-BureauKeys $Frame @('schema', 'type', 'nonce', 'sequence', 'guard', 'state', 'identity')
    if ($Frame.type -cnotmatch '\Achallenge\z' -or $Frame.nonce -isnot [string] -or
        $Frame.nonce -cnotmatch '\A[0-9a-f]{32}\z' -or $Frame.guard -isnot [string] -or
        $Frame.guard -cnotmatch '\A[0-9a-f]{32}\z' -or $Frame.guard -eq ('0' * 32)) {
        throw 'invalid-native-challenge'
    }
    Assert-BureauInteger $Frame.sequence 0 9007199254740991
    if ($null -ne $State.Sequence -and $Frame.sequence -ne ($State.Sequence + 1)) { throw 'challenge-sequence' }
    if ($null -ne $State.Guard -and $Frame.guard -cne $State.Guard) { throw 'guard-identity-changed' }
    $now = [Diagnostics.Stopwatch]::GetTimestamp()
    if ($State.LastChallengeTicks -gt 0 -and
        ($now - $State.LastChallengeTicks) * 1000.0 / [Diagnostics.Stopwatch]::Frequency -lt 500) {
        throw 'challenge-rate'
    }
    Assert-BureauNativeIdentity $Frame $State
    $State.Sequence = $Frame.sequence
    $State.Guard = $Frame.guard
    $State.LastChallengeTicks = $now
}

function Assert-BureauNativeIdentity {
    param($Frame, $State)
    if ($Frame.state -isnot [string] -or $Frame.state -cnotmatch '\A(starting|running)\z' -or
        ($State.Running -and $Frame.state -cne 'running')) {
        throw 'native-state-regression'
    }
    if ($null -eq $Frame.identity) {
        if ($Frame.state -ceq 'running' -or $null -ne $State.Identity) { throw 'unknown-native-identity' }
        return
    }
    $identity = Get-BureauIdentity $Frame.identity
    if ($null -ne $State.Identity -and $identity -cne $State.Identity) { throw 'native-identity-changed' }
    $State.Identity = $identity
    $State.Running = $Frame.state -ceq 'running'
}

function Get-BureauIdentity {
    param($Identity)
    Assert-BureauKeys $Identity @('invocation', 'pid', 'starttime', 'cgroup')
    if ($Identity.invocation -isnot [string] -or $Identity.invocation -cnotmatch '\A[0-9a-f]{32}\z' -or
        $Identity.invocation -eq ('0' * 32) -or $Identity.starttime -isnot [string] -or
        $Identity.starttime -cnotmatch '\A(0|[1-9][0-9]{0,19})\z' -or
        $Identity.cgroup -isnot [string] -or
        $Identity.cgroup -cnotmatch '\A/system\.slice/(?:[A-Za-z0-9_:@.-]+/)*[A-Za-z0-9_:@.-]+\.service\z') {
        throw 'invalid-native-identity'
    }
    Assert-BureauInteger $Identity.pid 1 2147483647
    $start = [ulong] 0
    if (![ulong]::TryParse($Identity.starttime, [ref] $start)) { throw 'invalid-native-starttime' }
    foreach ($part in $Identity.cgroup.Split('/')) {
        if ($part -cin @('.', '..')) { throw 'noncanonical-native-cgroup' }
    }
    return "$($Identity.invocation)|$($Identity.pid)|$($Identity.starttime)|$($Identity.cgroup)"
}

function New-BureauHeartbeat {
    param($Frame, [string] $Owner, [string] $Commit, [long] $Age)
    Assert-BureauInteger $Age 0 2000
    $message = @{ schema = 'bureau-windows-v1'; type = 'heartbeat'; nonce = $Frame.nonce;
        sequence = $Frame.sequence; guard = $Frame.guard; owner = $Owner; commit = $Commit; ageMs = $Age }
    return [Text.UTF8Encoding]::new($false, $true).GetBytes(($message | ConvertTo-Json -Compress) + "`n")
}

function New-BureauChannel {
    param([Diagnostics.Process] $Process)
    $channel = @{ Process = $Process; Buffer = [IO.MemoryStream]::new(); Frame = $null;
        Bytes = [byte[]]::new(1024); OutputOffset = 0; OutputCount = 0; Eof = $false;
        ErrorBytes = [byte[]]::new(1024); ErrorBuffer = [IO.MemoryStream]::new();
        ErrorEof = $false; ErrorBad = $false; OutputBad = $false;
        Fault = $null; OutputRead = $null; ErrorRead = $null }
    $channel.OutputRead = $Process.StandardOutput.BaseStream.ReadAsync($channel.Bytes, 0, 1024)
    $channel.ErrorRead = $Process.StandardError.BaseStream.ReadAsync($channel.ErrorBytes, 0, 1024)
    return $channel
}

function Add-BureauOutputByte {
    param($Channel, [byte] $Byte)
    if ($Byte -eq 10) {
        if ($null -ne $Channel.Frame) { throw 'pipelined-native-output' }
        $Channel.Frame = [Text.UTF8Encoding]::new($false, $true).GetString($Channel.Buffer.ToArray())
        $Channel.Buffer.SetLength(0)
        $Channel.Buffer.Position = 0
    } else {
        if ($Channel.Buffer.Length -ge 2047) { throw 'oversized-native-output' }
        $Channel.Buffer.WriteByte($Byte)
    }
}

function Receive-BureauOutputChunk {
    param($Channel)
    if ($Channel.OutputOffset -lt $Channel.OutputCount) { return $true }
    if ($null -eq $Channel.OutputRead -or !$Channel.OutputRead.IsCompleted) { return $false }
    $Channel.OutputCount = $Channel.OutputRead.GetAwaiter().GetResult()
    $Channel.OutputOffset = 0
    $Channel.OutputRead = $null
    if ($Channel.OutputCount -eq 0) {
        $Channel.Eof = $true
        if ($Channel.Buffer.Length -ne 0) { throw 'partial-native-frame' }
        return $false
    }
    return $true
}

function Update-BureauOutput {
    param($Channel)
    if ($null -ne $Channel.Frame -or $Channel.Eof -or $Channel.OutputBad) { return }
    if (!(Receive-BureauOutputChunk $Channel)) { return }
    while ($Channel.OutputOffset -lt $Channel.OutputCount) {
        Add-BureauOutputByte $Channel $Channel.Bytes[$Channel.OutputOffset]
        $Channel.OutputOffset++
        if ($null -ne $Channel.Frame) { break }
    }
    if ($Channel.OutputOffset -eq $Channel.OutputCount) {
        $Channel.OutputRead = $Channel.Process.StandardOutput.BaseStream.ReadAsync($Channel.Bytes, 0, 1024)
    }
}

function Update-BureauError {
    param($Channel)
    if ($null -eq $Channel.ErrorRead -or !$Channel.ErrorRead.IsCompleted) { return }
    $count = $Channel.ErrorRead.GetAwaiter().GetResult()
    $Channel.ErrorRead = $null
    if ($count -eq 0) {
        $Channel.ErrorEof = $true
        $null = [Text.UTF8Encoding]::new($false, $true).GetString($Channel.ErrorBuffer.ToArray())
        return
    }
    $Channel.Fault = 'native-stderr'
    if ($Channel.ErrorBuffer.Length + $count -gt 2048) { throw 'oversized-native-stderr' }
    $Channel.ErrorBuffer.Write($Channel.ErrorBytes, 0, $count)
    $Channel.ErrorRead = $Channel.Process.StandardError.BaseStream.ReadAsync($Channel.ErrorBytes, 0, 1024)
}

function Update-BureauChannel {
    param($Channel)
    try { Update-BureauOutput $Channel } catch {
        $Channel.Fault = 'native-output-refused'
        $Channel.OutputBad = $true
        $Channel.OutputRead = $null
    }
    try { Update-BureauError $Channel } catch {
        $Channel.Fault = 'native-stderr-refused'
        $Channel.ErrorBad = $true
        $Channel.ErrorRead = $null
    }
}
