[CmdletBinding()]
param([string] $Mode = 'run', [string] $CaseName = 'healthy', [string] $ReceiptPath, [string] $NodePath)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:Root = [IO.Path]::GetDirectoryName($PSScriptRoot)
$script:Windows = [IO.Path]::Combine($script:Root, 'deployment\windows')
$script:Pwsh = [Environment]::ProcessPath
$script:TestFile = $PSCommandPath
$script:Node = $NodePath
$script:Commit = '7de44252b9e4b7f73e0cf591f91c7a31f687343b'
$script:Guard = '11111111111111111111111111111111'
$script:Checks = 0

function Assert-Test {
    param([bool] $Condition, [string] $Name)
    if (!$Condition) { throw "offline assertion failed: $Name" }
    $script:Checks++
}

function New-TestProcessInfo {
    param([string[]] $Arguments)
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $script:Pwsh
    $info.WorkingDirectory = $script:Root
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.Environment.Clear()
    $info.Environment['SystemRoot'] = [IO.Path]::GetDirectoryName([Environment]::SystemDirectory)
    $info.Environment['WINDIR'] = $info.Environment['SystemRoot']
    $info.Environment['PATH'] = [Environment]::SystemDirectory
    foreach ($argument in (@('-NoProfile', '-NonInteractive', '-File') + $Arguments)) {
        $info.ArgumentList.Add($argument)
    }
    return $info
}

function Test-FixtureMutexHeld {
    $mutex = [Threading.Mutex]::new($false, 'Global\BureauMaintenanceOwner')
    try {
        $owned = $mutex.WaitOne(0)
        if ($owned) { $mutex.ReleaseMutex() }
        return !$owned
    } finally { $mutex.Dispose() }
}

function Write-FixtureBytes {
    param([byte[]] $Bytes)
    $script:FixtureOutput.Write($Bytes, 0, $Bytes.Length)
    $script:FixtureOutput.Flush()
}

function Write-FixtureFrame {
    param($Frame)
    Write-FixtureBytes ([Text.UTF8Encoding]::new($false).GetBytes(($Frame | ConvertTo-Json -Compress -Depth 5) + "`n"))
}

function New-FixtureChallenge {
    param([long] $Sequence, [string] $State = 'starting')
    $identity = $null
    if ($State -eq 'running') {
        $identity = @{ invocation = ('2' * 32); pid = 123; starttime = '987654';
            cgroup = '/system.slice/bureau-maintenance.service' }
    }
    return @{ schema = 'bureau-windows-v1'; type = 'challenge'; nonce = ('a' * 32);
        sequence = $Sequence; guard = $script:Guard; state = $State; identity = $identity }
}

function Read-FixtureReply {
    param($Client, $Challenge, [int] $Timeout = 10000)
    $read = [Console]::In.ReadLineAsync()
    if (!$read.Wait($Timeout)) { throw 'fixture-input-deadline' }
    $text = $read.GetAwaiter().GetResult()
    if ($null -eq $text) { $Client.Closed = $true; return $false }
    if ($null -eq $Challenge) { throw 'unexpected-heartbeat' }
    $reply = ConvertFrom-Json -InputObject $text -AsHashtable
    $Client.Count++
    if ($reply.Count -ne 8 -or $reply.schema -cne 'bureau-windows-v1' -or $reply.type -cne 'heartbeat' -or
        $reply.nonce -cne $Challenge.nonce -or $reply.sequence -ne $Challenge.sequence -or
        $reply.guard -cne $Challenge.guard -or $reply.commit -cne $script:Commit -or
        $reply.owner -cnotmatch '\A[0-9a-f]{32}\z' -or $reply.ageMs -lt 0 -or $reply.ageMs -gt 2000) {
        throw 'invalid-fixture-heartbeat'
    }
    if ($null -ne $Client.Owner -and $Client.Owner -cne $reply.owner) { throw 'changed-windows-owner' }
    $Client.Owner = $reply.owner
    return $true
}

function Send-FixtureChallenge {
    param($Client, $Challenge)
    Write-FixtureFrame $Challenge
    return Read-FixtureReply $Client $Challenge
}

function Wait-FixtureEof {
    param($Client, [int] $Timeout = 10000)
    if (!$Client.Closed) { $null = Read-FixtureReply $Client $null $Timeout }
    $Client.MutexHeld = $Client.MutexHeld -and (Test-FixtureMutexHeld)
}

function Write-FixtureReceipt {
    param($Client)
    if ($ReceiptPath) {
        [IO.File]::WriteAllText($ReceiptPath, ($Client | ConvertTo-Json -Compress),
            [Text.UTF8Encoding]::new($false))
    }
}

function Invoke-FixtureInvalidFirst {
    param($Client)
    $frame = New-FixtureChallenge 0
    switch ($CaseName) {
        'malformed' { Write-FixtureBytes ([Text.Encoding]::UTF8.GetBytes("{broken}`n")) }
        'oversized' {
            $json = ($frame | ConvertTo-Json -Compress).PadRight(2048)
            Write-FixtureBytes ([Text.Encoding]::UTF8.GetBytes($json + "`n"))
        }
        'partial' { Write-FixtureBytes ([Text.Encoding]::UTF8.GetBytes('{"schema"')); return }
        'utf8' { Write-FixtureBytes ([byte[]] @(195, 40, 10)) }
        'duplicate-key' {
            $json = ($frame | ConvertTo-Json -Compress) -replace '}\z', ',"sequence":0}'
            Write-FixtureBytes ([Text.Encoding]::UTF8.GetBytes($json + "`n"))
        }
        'unknown-key' { $frame.extra = $true; Write-FixtureFrame $frame }
        'wrong-schema' { $frame.schema = 'wrong'; Write-FixtureFrame $frame }
        'array-schema' { $frame.schema = @('bureau-windows-v1'); Write-FixtureFrame $frame }
        'array-type' { $frame.type = @('challenge'); Write-FixtureFrame $frame }
        'array-state' { $frame.state = @('starting'); Write-FixtureFrame $frame }
        'bad-nonce' { $frame.nonce = 'not-a-nonce'; Write-FixtureFrame $frame }
        'bad-sequence' { $frame.sequence = -1; Write-FixtureFrame $frame }
        'fractional-sequence' { $frame.sequence = 0.5; Write-FixtureFrame $frame }
        'running-null' { $frame.state = 'running'; Write-FixtureFrame $frame }
        'running-missing' { $frame.state = 'running'; $frame.identity = @{}; Write-FixtureFrame $frame }
        'bad-cgroup' {
            $frame = New-FixtureChallenge 0 'running'
            $frame.identity.cgroup = '/system.slice/../bureau-maintenance.service'
            Write-FixtureFrame $frame
        }
        'pipelined' {
            $one = (New-FixtureChallenge 0) | ConvertTo-Json -Compress
            $two = (New-FixtureChallenge 1) | ConvertTo-Json -Compress
            Write-FixtureBytes ([Text.Encoding]::UTF8.GetBytes("$one`n$two`n"))
        }
    }
    Wait-FixtureEof $Client
}

function Invoke-FixtureRunning {
    param($Client)
    [Threading.Thread]::Sleep(650)
    $frame = New-FixtureChallenge 1 'running'
    if ($CaseName -eq 'identity-lf') { $frame.identity.invocation += "`n" }
    if ($CaseName -eq 'identity-crlf') { $frame.identity.starttime += "`r`n" }
    if ($CaseName -eq 'identity-control') { $frame.identity.cgroup = "/system.slice/bureau$([char]0)-maintenance.service" }
    if (!(Send-FixtureChallenge $Client $frame)) {
        if ($CaseName -eq 'shutdown-identity') {
            [Threading.Thread]::Sleep(650)
            $changed = New-FixtureChallenge 2 'running'
            $changed.identity.pid = 456
            Write-FixtureFrame $changed
            [Threading.Thread]::Sleep(100)
        }
        return
    }
    if ($CaseName -eq 'loss-after-admission') { return }
    [Threading.Thread]::Sleep(650)
    $frame = New-FixtureChallenge 2 'running'
    if ($CaseName -eq 'identity-change') { $frame.identity.pid = 456 }
    if ($CaseName -eq 'state-regression') { $frame.state = 'starting' }
    $null = Send-FixtureChallenge $Client $frame
}

function Invoke-FixtureStarting {
    param($Client)
    foreach ($sequence in 1..3) {
        [Threading.Thread]::Sleep(1000)
        $state = 'starting'
        if ($sequence -eq 3) { $state = 'running' }
        if (!(Send-FixtureChallenge $Client (New-FixtureChallenge $sequence $state))) { return }
    }
}

function Invoke-FixtureExchange {
    param($Client)
    if ($CaseName -eq 'late-initial') { [Threading.Thread]::Sleep(1200) }
    if ($CaseName -like 'stderr*') {
        $data = [byte[]] @(195, 40)
        if ($CaseName -in @('stderr', 'stderr-zero-exit')) { $data = [Text.Encoding]::UTF8.GetBytes("native refusal`n") }
        if ($CaseName -eq 'stderr-flood') { $data = [byte[]]::new(2049) }
        [Console]::OpenStandardError().Write($data, 0, $data.Length)
        [Threading.Thread]::Sleep(50)
    }
    $first = New-FixtureChallenge 0
    if ($CaseName -eq 'frame-boundary') {
        $json = ($first | ConvertTo-Json -Compress).PadRight(2047)
        Write-FixtureBytes ([Text.Encoding]::UTF8.GetBytes($json + "`n"))
        if (!(Read-FixtureReply $Client $first)) { return }
    } elseif (!(Send-FixtureChallenge $Client $first)) { return }
    switch ($CaseName) {
        'healthy' { Invoke-FixtureRunning $Client }
        'continued-starting' { Invoke-FixtureStarting $Client }
        'identity-change' { Invoke-FixtureRunning $Client }
        'identity-lf' { Invoke-FixtureRunning $Client }
        'identity-crlf' { Invoke-FixtureRunning $Client }
        'identity-control' { Invoke-FixtureRunning $Client }
        'state-regression' { Invoke-FixtureRunning $Client }
        'shutdown-identity' { Invoke-FixtureRunning $Client }
        'loss-after-admission' { Invoke-FixtureRunning $Client }
        'stall-after-challenge' { Wait-FixtureEof $Client }
        'burst' { $null = Send-FixtureChallenge $Client (New-FixtureChallenge 1) }
        'sequence-replay' {
            [Threading.Thread]::Sleep(650)
            $null = Send-FixtureChallenge $Client (New-FixtureChallenge 0)
        }
        'guard-change' {
            [Threading.Thread]::Sleep(650)
            $frame = New-FixtureChallenge 1
            $frame.guard = ('3' * 32)
            $null = Send-FixtureChallenge $Client $frame
        }
    }
}

function Invoke-FixtureEarlyRefusal {
    param($Client)
    [Console]::Error.WriteLine('native refusal')
    Write-FixtureFrame @{ schema = 'bureau-windows-v1'; type = 'stopped'; drained = $true }
    $Client.StopSent = $true
    Wait-FixtureEof $Client
}

function Invoke-TestClient {
    $script:FixtureOutput = [Console]::OpenStandardOutput()
    $client = @{ Count = 0; Owner = $null; Closed = $false; Valid = $true;
        MutexHeld = (Test-FixtureMutexHeld); CleanEnvironment = $true; StopSent = $false; ExitCode = 0 }
    $code = 0
    foreach ($value in [Environment]::GetEnvironmentVariables().Values) {
        if ($value -ceq 'offline-inherited-must-not-escape') { $client.CleanEnvironment = $false }
    }
    try {
        if ($CaseName -eq 'eof') { return }
        if ($CaseName -eq 'no-challenge') { Wait-FixtureEof $client 23000; $code = 1; return }
        if ($CaseName -eq 'early-refusal') { Invoke-FixtureEarlyRefusal $client; $code = 1; return }
        $invalid = @('malformed', 'oversized', 'partial', 'utf8', 'duplicate-key', 'unknown-key',
            'wrong-schema', 'array-schema', 'array-type', 'array-state', 'bad-nonce', 'bad-sequence', 'fractional-sequence', 'running-null',
            'running-missing', 'bad-cgroup', 'pipelined')
        if ($CaseName -in $invalid) { Invoke-FixtureInvalidFirst $client; $code = 1; return }
        Invoke-FixtureExchange $client
        if ($CaseName -eq 'loss-after-admission') { return }
        if ($CaseName -eq 'missing-drain') { $code = 1; return }
        if ($client.Closed) { $code = 1 }
        $stopped = @{ schema = 'bureau-windows-v1'; type = 'stopped'; drained = $true }
        if ($CaseName -eq 'stopped-false') { $stopped.drained = $false }
        if ($CaseName -eq 'stopped-extra') { $stopped.extra = 1 }
        if ($CaseName -eq 'malformed-drain') { $stopped.drained = 'true'; $code = 1 }
        Write-FixtureFrame $stopped
        $client.StopSent = $true
        if ($CaseName -eq 'post-stopped-output') {
            [Threading.Thread]::Sleep(100)
            Write-FixtureBytes ([Text.Encoding]::UTF8.GetBytes("unexpected`n"))
        }
        if ($CaseName -eq 'drain-stall') {
            Write-FixtureReceipt $client
            [Threading.Thread]::Sleep(40000)
        }
        Wait-FixtureEof $client
        if ($CaseName -eq 'nonzero-delayed') {
            [Threading.Thread]::Sleep(650)
            $client.MutexHeld = $client.MutexHeld -and (Test-FixtureMutexHeld)
        }
        if ($CaseName -in @('nonzero', 'nonzero-delayed') -or $CaseName -like 'stderr*') { $code = 1 }
        if ($CaseName -eq 'stderr-zero-exit') { $code = 0 }
    } catch { $client.Valid = $false; $code = 91 } finally {
        $client.ExitCode = $code
        Write-FixtureReceipt $client
        if ($code -ne 0) { exit $code }
    }
}

function Invoke-TestMutexHolder {
    $mutex = [Threading.Mutex]::new($false, 'Global\BureauMaintenanceOwner')
    $owned = $false
    try {
        $owned = $mutex.WaitOne(0)
        if (!$owned) { throw 'fixture-mutex-unavailable' }
        [Console]::WriteLine('locked')
        if (![Console]::In.ReadLineAsync().Wait(10000)) { throw 'fixture-mutex-deadline' }
    } finally {
        if ($owned) { $mutex.ReleaseMutex() }
        $mutex.Dispose()
    }
}

function New-TestConfig {
    $config = Read-BureauJson ([IO.File]::ReadAllText([IO.Path]::Combine($script:Windows, 'config.example.json')))
    $config.approved = $true
    $config.qualified = $true
    $config.commit = $script:Commit
    $config.vhdIdentity.fileId = ('a' * 32)
    $config.vhdIdentity.volumeSerial = '0123456789abcdef'
    foreach ($name in @($config.sourceSha256.Keys)) {
        $bytes = [IO.File]::ReadAllBytes([IO.Path]::Combine($script:Windows, $name))
        $config.sourceSha256[$name] = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
    }
    return $config
}

function New-TestSample {
    param($Config)
    $startupMemory = Get-BureauStartupMemoryMinimum $script:NativeBudget $Config.limits
    return @{ WindowsUserSid = $Config.windowsUserSid; Distro = $Config.distro;
        RegistrationId = $Config.registrationId; RegistrationStamp = '0123456789abcdef'; Version = 2;
        BasePath = 'D:\BureauOperations'; VhdFileName = 'ext4.vhdx'; FinalPath = $Config.vhdPath;
        VolumePath = $Config.vhdIdentity.volumePath; VolumeSerial = $Config.vhdIdentity.volumeSerial;
        FileId = $Config.vhdIdentity.fileId; PhysicalBytes = [long] 13958643712; LogicalBytes = [long] 13958643712;
        BackingFreeBytes = [long] 17179869184; MemoryFreeBytes = $startupMemory;
        Attributes = [uint32] 32; Links = [uint32] 1; DeletePending = $false;
        FileType = [uint32] 1; DriveType = [uint32] 3; StartedTicks = [long] 0; FinishedTicks = [long] 0 }
}

function New-TestProbe {
    param($Config, [string] $Behavior = 'healthy', [int] $ChangeAt = 1, $Mutation = @{})
    $probe = [pscustomobject] @{ Template = (New-TestSample $Config); Behavior = $Behavior;
        ChangeAt = $ChangeAt; Mutation = $Mutation; Count = 0; Disposed = $false; DelayMilliseconds = 0 }
    $probe | Add-Member ScriptMethod SampleAsync {
        $this.Count++
        $pending = [Threading.Tasks.TaskCompletionSource[object]]::new()
        $sample = $this.Template.Clone()
        $sample.StartedTicks = [Diagnostics.Stopwatch]::GetTimestamp()
        if ($this.Count -ge $this.ChangeAt) {
            switch ($this.Behavior) {
                'throw' { throw 'injected-resource-api-failure' }
                'freeze' { return $pending.Task }
                'slow' { return [Threading.Tasks.Task]::Delay(2100) }
                'stale' { $sample.StartedTicks -= [Diagnostics.Stopwatch]::Frequency * 3 }
            }
            foreach ($key in $this.Mutation.Keys) { $sample[$key] = $this.Mutation[$key] }
        }
        if ($this.DelayMilliseconds -gt 0) { [Threading.Thread]::Sleep($this.DelayMilliseconds) }
        $sample.FinishedTicks = [Diagnostics.Stopwatch]::GetTimestamp()
        $pending.SetResult([pscustomobject] $sample)
        return $pending.Task
    }
    $probe | Add-Member ScriptMethod Dispose { $this.Disposed = $true }
    return $probe
}

function New-TestEffects {
    param($Probe, [string] $Fixture, [string] $Receipt)
    $record = @{ Starts = 0; Info = $null }
    $fixtureInfo = New-TestProcessInfo @($script:TestFile, '-Mode', 'client',
        '-CaseName', $Fixture, '-ReceiptPath', $Receipt)
    # Real-clock drain cases cover premature release and the 25-second fallbacks.
    $factor = 20
    if ($Fixture -in @('drain-stall', 'loss-after-admission', 'nonzero-delayed')) { $factor = 1 }
    $clock = {
        $value = [pscustomobject] @{ Watch = [Diagnostics.Stopwatch]::StartNew(); Factor = $factor }
        $value | Add-Member ScriptProperty ElapsedMilliseconds { [long] ($this.Watch.ElapsedMilliseconds * $this.Factor) }
        return $value
    }.GetNewClosure()
    $start = {
        param($info)
        $record.Starts++
        $record.Info = $info
        $fixtureInfo.Environment.Clear()
        foreach ($pair in $info.Environment.GetEnumerator()) { $fixtureInfo.Environment[$pair.Key] = $pair.Value }
        $fixtureInfo.UseShellExecute = $info.UseShellExecute
        $fixtureInfo.RedirectStandardInput = $info.RedirectStandardInput
        $fixtureInfo.RedirectStandardOutput = $info.RedirectStandardOutput
        $fixtureInfo.RedirectStandardError = $info.RedirectStandardError
        $fixtureInfo.StandardInputEncoding = $info.StandardInputEncoding
        $fixtureInfo.StandardOutputEncoding = $info.StandardOutputEncoding
        $fixtureInfo.StandardErrorEncoding = $info.StandardErrorEncoding
        return [Diagnostics.Process]::Start($fixtureInfo)
    }.GetNewClosure()
    return @{ NewProbe = { param($config) $Probe }.GetNewClosure(); StartClient = $start;
        StopRequested = { $Probe.Behavior -eq 'interrupt' -and $Probe.Count -ge 2 }.GetNewClosure();
        NewDrainClock = $clock; Record = $record }
}

function Invoke-TestCase {
    param([string] $Name, [string] $Fixture = 'single', [bool] $Success = $false,
        [int] $Beats = 0, [string] $Behavior = 'healthy', [int] $ChangeAt = 1, $Mutation = @{},
        [bool] $Spawned = $true, [bool] $Drained = $false, $NativeBudget = $script:NativeBudget)
    $config = New-TestConfig
    $probe = New-TestProbe $config $Behavior $ChangeAt $Mutation
    $receipt = [IO.Path]::Combine($script:Scratch, "$Name.json")
    $effects = New-TestEffects $probe $Fixture $receipt
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $result = Invoke-BureauSupervisor $config $effects $NativeBudget
    Assert-Test ($result.Success -eq $Success -and $result.Spawned -eq $Spawned -and
        $result.Drained -eq $Drained) "$Name result"
    Assert-Test ($effects.Record.Starts -eq [int] $Spawned) "$Name never restarts"
    if ($Spawned) {
        $client = Read-BureauJson ([IO.File]::ReadAllText($receipt))
        Assert-Test ($client.Count -eq $Beats -and $client.Valid -and $client.MutexHeld -and
            $client.CleanEnvironment) "$Name heartbeat and owned lifetime"
        if ($Fixture -notin @('drain-stall', 'loss-after-admission', 'missing-drain', 'eof', 'partial')) {
            Assert-Test $client.Closed "$Name closes stdin"
        }
    }
    [Console]::WriteLine("PASS $Name")
    return @{ Result = $result; Config = $config; Probe = $probe; Effects = $effects; Elapsed = $clock.ElapsedMilliseconds;
        Receipt = $receipt }
}

function Test-StartupReserveAgain {
    param($Config, $Mutation)
    $probe = New-TestProbe $Config 'healthy' 1 $Mutation
    $effects = New-TestEffects $probe 'single' ''
    $result = Invoke-BureauSupervisor $Config $effects $script:NativeBudget
    Assert-Test (!$result.Spawned -and !$result.Success -and $effects.Record.Starts -eq 0 -and
        $probe.Count -eq 1 -and $Config.approved -and $Config.qualified) 'persisted approvals cannot bypass fresh startup reserve'
}

function Test-TightenedStartupReserve {
    $config = New-TestConfig
    $config.limits.vhdBytesMax = [long] 15032385535
    $probe = New-TestProbe $config
    $effects = New-TestEffects $probe 'single' ''
    $result = Invoke-BureauSupervisor $config $effects $script:NativeBudget
    Assert-Test (!$result.Spawned -and !$result.Success -and $effects.Record.Starts -eq 0 -and
        $probe.Count -eq 1) 'tightened stop retains the full startup reserve'
}

function Test-StartupReserve {
    $boundary = @{ PhysicalBytes = [long] 13958643712; LogicalBytes = [long] 13958643712 }
    $allowed = Invoke-TestCase -Name 'startup-reserve-boundary' -Mutation $boundary -Success $true -Beats 1 -Drained $true
    foreach ($field in @('PhysicalBytes', 'LogicalBytes')) {
        $null = Invoke-TestCase -Name "startup-$field-above-boundary" -Mutation @{ $field = [long] 13958643713 } -Spawned $false
        $null = Invoke-TestCase -Name "initial-heartbeat-$field-above-boundary" -Mutation @{ $field = [long] 13958643713 } -ChangeAt 2 -Drained $true
    }
    $current = @{ PhysicalBytes = [long] 14877196288; LogicalBytes = [long] 14877196288 }
    $null = Invoke-TestCase -Name 'startup-current-size-refused' -Mutation $current -Spawned $false
    Test-StartupReserveAgain $allowed.Config $current
    Test-TightenedStartupReserve
    $null = Invoke-TestCase -Name 'initial-heartbeat-vhd-boundary' -Mutation $boundary -ChangeAt 2 -Success $true -Beats 1 -Drained $true
    $null = Invoke-TestCase -Name 'initial-heartbeat-current-vhd-refused' -Mutation $current -ChangeAt 2 -Drained $true
    $null = Invoke-TestCase -Name 'continuous-reserve-consumed' -Fixture 'healthy' -Mutation $current -ChangeAt 3 -Success $true -Beats 3 -Drained $true
    foreach ($field in @('PhysicalBytes', 'LogicalBytes')) {
        $null = Invoke-TestCase -Name "continuous-$field-stop" -Fixture 'healthy' -Mutation @{ $field = [long] 15032385536 } -ChangeAt 3 -Beats 1 -Drained $true
    }
}

function New-TestNativeBudget {
    $copy = @{}
    foreach ($key in $script:NativeBudget.Keys) { $copy[$key] = $script:NativeBudget[$key] }
    return $copy
}

function Test-StartupMemory {
    $boundary = @{ MemoryFreeBytes = [long] 9797894144 }
    $below = @{ MemoryFreeBytes = [long] 9797894143 }
    $floor = @{ MemoryFreeBytes = [long] 1073741825 }
    Assert-Test ((Get-BureauStartupMemoryMinimum $script:NativeBudget $script:Config.limits) -eq 9797894144) 'shared native budget derives admission memory'
    $allowed = Invoke-TestCase -Name 'startup-memory-boundary' -Mutation $boundary -Success $true -Beats 1 -Drained $true
    $null = Invoke-TestCase -Name 'startup-memory-below-boundary' -Mutation $below -Spawned $false
    $null = Invoke-TestCase -Name 'startup-memory-barely-above-floor' -Mutation $floor -Spawned $false
    Test-StartupReserveAgain $allowed.Config $below
    $null = Invoke-TestCase -Name 'initial-heartbeat-memory-boundary' -Mutation $boundary -ChangeAt 2 -Success $true -Beats 1 -Drained $true
    $fresh = Invoke-TestCase -Name 'initial-heartbeat-memory-refused' -Mutation $below -ChangeAt 2 -Drained $true
    Assert-Test ($fresh.Probe.Count -ge 2) 'initial admission uses a new resource sample'
    $null = Invoke-TestCase -Name 'initial-heartbeat-memory-barely-above-floor' -Mutation $floor -ChangeAt 2 -Drained $true
    $null = Invoke-TestCase -Name 'continuous-memory-floor' -Fixture 'healthy' -Mutation @{ MemoryFreeBytes = [long] 1073741824 } -ChangeAt 3 -Success $true -Beats 3 -Drained $true
    $null = Invoke-TestCase -Name 'continuous-memory-below-floor' -Fixture 'healthy' -Mutation @{ MemoryFreeBytes = [long] 1073741823 } -ChangeAt 3 -Beats 1 -Drained $true
}

function Test-DerivedNativeMemory {
    foreach ($field in @('engineMemoryMaxBytes', 'guardianMemoryMaxBytes')) {
        $budget = New-TestNativeBudget
        $budget[$field]++
        $null = Invoke-TestCase -Name "derived-$field" -NativeBudget $budget -Spawned $false
    }
    $config = New-TestConfig
    $config.limits.memoryFreeBytesMin = [long] 2147483648
    $probe = New-TestProbe $config
    $probe.Template.MemoryFreeBytes = [long] 9797894144
    $effects = New-TestEffects $probe 'single' ''
    $result = Invoke-BureauSupervisor $config $effects $script:NativeBudget
    Assert-Test (!$result.Spawned -and !$result.Success -and $effects.Record.Starts -eq 0 -and
        $probe.Count -eq 1) 'startup memory includes the configured continuous floor'
}

function Test-NativeBudgetValidation {
    $cases = @(@{ schema = 'wrong' }, @{ schema = @('bureau-native-budget-v1') },
        @{ engineMemoryMaxBytes = 0 }, @{ guardianMemoryMaxBytes = $null },
        @{ engineMemoryMaxBytes = 1.5 }, @{ guardianMemoryMaxBytes = -1 },
        @{ extra = 1 }, @{ engineMemoryMaxBytes = [long]::MaxValue })
    $index = 0
    foreach ($case in $cases) {
        $budget = New-TestNativeBudget
        foreach ($key in $case.Keys) { $budget[$key] = $case[$key] }
        $null = Invoke-TestCase -Name "invalid-native-budget-$index" -NativeBudget $budget -Spawned $false
        $index++
    }
    foreach ($kind in @('missing', 'mismatched')) {
        $config = New-TestConfig
        if ($kind -eq 'missing') { $config.sourceSha256.Remove('native-budget.json') }
        else { $config.sourceSha256['native-budget.json'] = ('0' * 64) }
        $path = [IO.Path]::Combine($script:Scratch, "budget-pin-$kind.json")
        [IO.File]::WriteAllText($path, ($config | ConvertTo-Json -Depth 8))
        $refused = $false
        try { $null = Read-BureauBundle $path } catch { $refused = $true }
        Assert-Test $refused "$kind native budget source pin refuses before loading"
    }
}

function Test-PreSpawnPolicy {
    $cases = @(
        @{ Name = 'physical-cap'; PhysicalBytes = [long] 15032385536 },
        @{ Name = 'logical-cap'; LogicalBytes = [long] 15032385536 },
        @{ Name = 'backing-free'; BackingFreeBytes = [long] 8589934591 },
        @{ Name = 'memory-free'; MemoryFreeBytes = [long] 1073741823 },
        @{ Name = 'missing-metric'; PhysicalBytes = $null },
        @{ Name = 'missing-logical'; LogicalBytes = $null },
        @{ Name = 'missing-backing-free'; BackingFreeBytes = $null },
        @{ Name = 'missing-memory'; MemoryFreeBytes = $null },
        @{ Name = 'zero-metric'; BackingFreeBytes = [long] 0 },
        @{ Name = 'wrong-distro'; Distro = 'OtherDistribution' },
        @{ Name = 'wrong-user'; WindowsUserSid = 'S-1-5-21-1-2-3-1001' },
        @{ Name = 'wrong-registration'; RegistrationId = '{aaaaaaaa-2222-3333-4444-555555555555}' },
        @{ Name = 'wrong-base'; BasePath = 'D:\OtherDistribution' },
        @{ Name = 'wrong-vhd-name'; VhdFileName = 'other.vhdx' },
        @{ Name = 'wrong-final-path'; FinalPath = 'D:\OtherDistribution\ext4.vhdx' },
        @{ Name = 'wrong-volume'; VolumeSerial = 'aaaaaaaaaaaaaaaa' },
        @{ Name = 'wrong-volume-guid'; VolumePath = '\\?\Volume{aaaaaaaa-2222-3333-4444-555555555555}\' },
        @{ Name = 'wrong-file'; FileId = ('b' * 32) },
        @{ Name = 'reparse-file'; Attributes = [uint32] 1024 },
        @{ Name = 'ambiguous-hardlink'; Links = [uint32] 2 },
        @{ Name = 'missing-attributes'; Attributes = $null },
        @{ Name = 'missing-file-state'; DeletePending = $null },
        @{ Name = 'invalid-vhd-leaf'; VhdFileName = '..\ext4.vhdx' },
        @{ Name = 'invalid-base-path'; BasePath = 'D:\BureauOperations\..' }
    )
    foreach ($case in $cases) {
        $name = $case.Name
        $case.Remove('Name')
        $null = Invoke-TestCase -Name $name -Mutation $case -Spawned $false
    }
    foreach ($behavior in @('throw', 'stale')) {
        $null = Invoke-TestCase -Name "preflight-$behavior" -Behavior $behavior -Spawned $false
    }
}

function Test-ResourceLayouts {
    $sizes = @{ Identity = 24; Standard = 24; Compression = 16; Attributes = 8; Memory = 64 }
    $assembly = [Bureau.Windows.HostProbe].Assembly
    $sizeOf = [Runtime.InteropServices.Marshal].GetMethod('SizeOf', [type[]] @([type]))
    foreach ($name in $sizes.Keys) {
        $type = $assembly.GetType("Bureau.Windows.HostNative+$name", $true)
        Assert-Test ($sizeOf.Invoke($null, [object[]] @($type)) -eq $sizes[$name]) "$name Win32 layout"
    }
    $standard = $assembly.GetType('Bureau.Windows.HostNative+Standard', $true)
    $compression = $assembly.GetType('Bureau.Windows.HostNative+Compression', $true)
    Assert-Test ([Runtime.InteropServices.Marshal]::OffsetOf($standard, 'Eof').ToInt64() -eq 8 -and
        [Runtime.InteropServices.Marshal]::OffsetOf($compression, 'Physical').ToInt64() -eq 0) 'distinct physical allocation and logical EOF fields'
}

function Get-TestInvalidText {
    param([string] $Value)
    return @("$Value`n", "$Value`r`n", $Value.Insert(1, "`n"), $Value.Insert(1, "`t"),
        $Value.Insert(1, [string] [char] 0), $Value.Insert(1, [string] [char] 127),
        $Value.Insert(1, [string] [char] 133))
}

function Assert-TestRefuses {
    param([scriptblock] $Action, [string] $Name)
    $refused = $false
    try { $null = & $Action } catch { $refused = $true }
    Assert-Test $refused $Name
}

function Test-ExactProtocolText {
    $baseline = (New-FixtureChallenge 0 'running').identity
    Assert-Test ((Get-BureauIdentity $baseline).Contains('|987654|')) 'canonical identity accepted'
    foreach ($field in @('invocation', 'starttime', 'cgroup')) {
        foreach ($value in (Get-TestInvalidText $baseline[$field])) {
            $identity = $baseline.Clone()
            $identity[$field] = $value
            Assert-TestRefuses { Get-BureauIdentity $identity } "$field rejects trailing and embedded controls"
        }
    }
    $baseline = New-FixtureChallenge 0
    foreach ($field in @('schema', 'type', 'nonce', 'guard', 'state')) {
        foreach ($value in (Get-TestInvalidText $baseline[$field])) {
            $frame = $baseline.Clone()
            $frame[$field] = $value
            $text = $frame | ConvertTo-Json -Compress
            Assert-TestRefuses { Read-BureauFrame $text (New-BureauProtocol) } "$field has an exact protocol boundary"
        }
    }
}

function Test-ExactConfigText {
    $baseline = New-TestConfig
    foreach ($field in @('schema', 'windowsUserSid', 'registrationId', 'distro', 'commit', 'vhdPath')) {
        foreach ($value in (Get-TestInvalidText $baseline[$field])) {
            $config = Read-BureauJson ($baseline | ConvertTo-Json -Depth 8 -Compress)
            $config[$field] = $value
            Assert-TestRefuses { Assert-BureauConfig $config } "$field has an exact configuration boundary"
        }
    }
    foreach ($field in @('volumePath', 'volumeSerial', 'fileId')) {
        foreach ($value in (Get-TestInvalidText $baseline.vhdIdentity[$field])) {
            $config = Read-BureauJson ($baseline | ConvertTo-Json -Depth 8 -Compress)
            $config.vhdIdentity[$field] = $value
            Assert-TestRefuses { Assert-BureauConfig $config } "$field has an exact pin boundary"
        }
    }
    foreach ($value in (Get-TestInvalidText $script:NativeBudget.schema)) {
        $budget = New-TestNativeBudget
        $budget.schema = $value
        Assert-TestRefuses { Assert-BureauNativeBudget $budget } 'native budget schema rejects controls'
    }
}

function Test-ExactResourceText {
    $config = New-TestConfig
    $baseline = New-TestSample $config
    foreach ($field in @('BasePath', 'VhdFileName', 'RegistrationStamp')) {
        foreach ($value in (Get-TestInvalidText $baseline[$field])) {
            $sample = $baseline.Clone()
            $sample[$field] = $value
            Assert-TestRefuses { Assert-BureauRegistration $sample $config } "$field rejects controls"
        }
    }
    $flags = [Reflection.BindingFlags]::NonPublic -bor [Reflection.BindingFlags]::Static
    $paths = @{ NormalPath = $config.vhdPath; VolumeRoot = ($config.vhdIdentity.volumePath + 'BureauOperations\ext4.vhdx') }
    foreach ($name in $paths.Keys) {
        $method = [Bureau.Windows.HostProbe].GetMethod($name, $flags)
        foreach ($value in (Get-TestInvalidText $paths[$name])) {
            Assert-TestRefuses { $method.Invoke($null, [object[]] @($value)) } "$name rejects controls in the real helper"
        }
    }
}

function Test-ExactHashAndTaskText {
    $baseline = New-TestConfig
    $supervisor = [IO.Path]::Combine($script:Windows, 'supervise.ps1')
    $path = [IO.Path]::Combine($script:Scratch, 'control-hash.json')
    $locks = [Collections.Generic.List[IDisposable]]::new()
    try {
        foreach ($value in (Get-TestInvalidText $baseline.sourceSha256['supervise.ps1'])) {
            Assert-TestRefuses { Read-BureauTaskPin $supervisor $value $locks } 'task digest rejects controls'
            $config = Read-BureauJson ($baseline | ConvertTo-Json -Depth 8 -Compress)
            $config.sourceSha256['supervise.ps1'] = $value
            [IO.File]::WriteAllText($path, ($config | ConvertTo-Json -Depth 8))
            Assert-TestRefuses { Read-BureauBundle $path } 'bundle digest rejects controls'
        }
        foreach ($value in (Get-TestInvalidText $supervisor)) {
            Assert-TestRefuses { Assert-BureauTaskPath $value } 'task path rejects controls'
        }
        foreach ($value in (Get-TestInvalidText $baseline.windowsUserSid)) {
            Assert-TestRefuses { New-BureauLogonTaskXml $value $script:Pwsh $supervisor $script:ConfigPath } 'task SID rejects controls'
        }
    } finally { foreach ($file in $locks) { $file.Dispose() } }
}

function Test-ProtocolFailures {
    foreach ($name in @('eof', 'partial', 'malformed', 'oversized', 'utf8', 'duplicate-key', 'unknown-key',
        'wrong-schema', 'array-schema', 'array-type', 'array-state', 'bad-nonce', 'bad-sequence', 'fractional-sequence', 'running-null',
        'running-missing', 'bad-cgroup', 'pipelined', 'stderr-flood', 'stderr-malformed')) {
        $null = Invoke-TestCase -Name $name -Fixture $name
    }
    foreach ($name in @('burst', 'sequence-replay', 'guard-change', 'stopped-false', 'stopped-extra',
        'post-stopped-output', 'identity-lf', 'identity-crlf', 'identity-control')) {
        $null = Invoke-TestCase -Name $name -Fixture $name -Beats 1
    }
    foreach ($name in @('identity-change', 'state-regression')) {
        $null = Invoke-TestCase -Name $name -Fixture $name -Beats 2
    }
}

function Test-DrainEvidence {
    foreach ($name in @('nonzero', 'nonzero-delayed', 'stderr', 'stderr-zero-exit', 'early-refusal')) {
        $beats = 0
        if ($name -in @('nonzero', 'nonzero-delayed')) { $beats = 1 }
        $test = Invoke-TestCase -Name $name -Fixture $name -Beats $beats -Drained $true
        $receipt = Read-BureauJson ([IO.File]::ReadAllText($test.Receipt))
        $code = 1
        if ($name -eq 'stderr-zero-exit') { $code = 0 }
        Assert-Test ($receipt.StopSent -and $receipt.ExitCode -eq $code) "$name retains failure with confirmed drain"
        Assert-Test ($test.Elapsed -lt 6000) "$name does not wait for the 25-second fallback"
        if ($name -eq 'nonzero-delayed') { Assert-Test ($test.Elapsed -ge 650) 'receipt alone cannot release the owner before client exit' }
    }
    foreach ($name in @('missing-drain', 'malformed-drain')) {
        $test = Invoke-TestCase -Name $name -Fixture $name -Beats 1
        $receipt = Read-BureauJson ([IO.File]::ReadAllText($test.Receipt))
        Assert-Test ($receipt.ExitCode -eq 1) "$name nonzero producer cannot establish drain"
    }
}

function New-TestNativeEffects {
    param($Probe, $Config, [string] $FixtureMode, [int] $StartupDelayMilliseconds = 0)
    $record = @{ Starts = 0; Process = $null; ExitCode = $null; DrainClock = $null }
    $native = [Diagnostics.ProcessStartInfo]::new()
    $native.FileName = $script:Node
    $native.WorkingDirectory = $script:Root
    if ($StartupDelayMilliseconds -gt 0) {
        $delay = "await new Promise(resolve => setTimeout(resolve, $StartupDelayMilliseconds));"
        $native.ArgumentList.Add('--import')
        $native.ArgumentList.Add('data:text/javascript,' + [Uri]::EscapeDataString($delay))
    }
    $fixture = [IO.Path]::Combine($script:Root, 'scripts\supervision-drain-fixture.mjs')
    foreach ($value in @($fixture, 'client', $Config.commit, $FixtureMode)) { $native.ArgumentList.Add($value) }
    $start = {
        param($info)
        $record.Starts++
        $native.UseShellExecute = $info.UseShellExecute
        $native.RedirectStandardInput = $info.RedirectStandardInput
        $native.RedirectStandardOutput = $info.RedirectStandardOutput
        $native.RedirectStandardError = $info.RedirectStandardError
        $native.StandardInputEncoding = $info.StandardInputEncoding
        $native.StandardOutputEncoding = $info.StandardOutputEncoding
        $native.StandardErrorEncoding = $info.StandardErrorEncoding
        $native.Environment.Clear()
        foreach ($pair in $info.Environment.GetEnumerator()) { $native.Environment[$pair.Key] = $pair.Value }
        $record.Process = [Diagnostics.Process]::Start($native)
        return $record.Process
    }.GetNewClosure()
    $drain = {
        $record.DrainClock = [Diagnostics.Stopwatch]::StartNew()
        return $record.DrainClock
    }.GetNewClosure()
    return @{ NewProbe = { param($config) $Probe }.GetNewClosure(); StartClient = $start;
        StopRequested = { $false }; NewDrainClock = $drain; Record = $record }
}

function Get-TestNodeVersion {
    $info = New-BureauStartInfo (New-TestConfig)
    $info.FileName = $script:Node
    $info.ArgumentList.Clear()
    $info.ArgumentList.Add('--version')
    $process = [Diagnostics.Process]::Start($info)
    $output = $process.StandardOutput.ReadToEndAsync()
    $errors = $process.StandardError.ReadToEndAsync()
    try {
        if (!$process.WaitForExit(5000)) { throw 'Node version probe exceeded five seconds' }
        $version = $output.GetAwaiter().GetResult().Trim()
        Assert-Test ($process.ExitCode -eq 0 -and $version.Length -gt 0 -and
            $errors.GetAwaiter().GetResult().Length -eq 0) 'tested Node reports its version'
        return $version
    } finally {
        if (!$process.HasExited) { $process.Kill(); $null = $process.WaitForExit(1000) }
        $process.Dispose()
    }
}

function Test-NativeDrainCase {
    param([string] $FixtureMode, [int] $StartupDelayMilliseconds = 0)
    $config = New-TestConfig
    $probe = New-TestProbe $config 'throw' 4
    # The producer uses 20 ms intervals; real samples still take less than the 2-second bound.
    $probe.DelayMilliseconds = 650
    $effects = New-TestNativeEffects $probe $config $FixtureMode $StartupDelayMilliseconds
    $probe | Add-Member NoteProperty NativeRecord $effects.Record
    $probe | Add-Member ScriptMethod Dispose {
        $this.Disposed = $true
        $this.NativeRecord.ExitCode = $this.NativeRecord.Process.ExitCode
    } -Force
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $result = Invoke-BureauSupervisor $config $effects $script:NativeBudget
    $totalMilliseconds = $clock.ElapsedMilliseconds
    $drainMilliseconds = $null
    if ($null -ne $effects.Record.DrainClock) { $drainMilliseconds = $effects.Record.DrainClock.ElapsedMilliseconds }
    [Console]::WriteLine("TIMING actual-native-$FixtureMode startupDelayMs=$StartupDelayMilliseconds totalMs=$totalMilliseconds drainMs=$drainMilliseconds")
    Assert-Test ($result.Spawned -and !$result.Success -and
        $result.Drained -eq ($FixtureMode -eq 'drained')) "actual native $FixtureMode drain evidence with retained failure"
    Assert-Test ($effects.Record.Starts -eq 1 -and $effects.Record.ExitCode -eq 1) "actual native $FixtureMode exits one without restart"
    Assert-Test ($probe.Count -ge 4) "actual native $FixtureMode accepted starting and running heartbeats before EOF"
    Assert-Test ($null -ne $drainMilliseconds) 'actual supervisor drain clock was recorded'
    if ($FixtureMode -eq 'drained') { Assert-Test ($drainMilliseconds -lt 10000) 'actual confirmed drain skips the fallback' }
    else { Assert-Test ($drainMilliseconds -ge 25000 -and $drainMilliseconds -lt 35000) 'actual missing proof retains the owner through the real drain deadline' }
    if ($StartupDelayMilliseconds -gt 0) { Assert-Test ($totalMilliseconds -gt 10000) 'permitted slow startup does not consume the confirmed-drain budget' }
    [Console]::WriteLine("PASS actual-native-$FixtureMode startupDelayMs=$StartupDelayMilliseconds")
}

function Test-NativeDrainProducer {
    Assert-Test ([IO.Path]::IsPathFullyQualified($script:Node) -and [IO.File]::Exists($script:Node)) 'absolute installed Node is available'
    [Console]::WriteLine("NODE executable=$script:Node version=$(Get-TestNodeVersion)")
    Test-NativeDrainCase 'drained'
    Test-NativeDrainCase 'unconfirmed'
    Test-NativeDrainCase 'drained' 8500
}

function Test-MonitorFailures {
    foreach ($behavior in @('throw', 'freeze', 'slow', 'stale', 'interrupt')) {
        $test = Invoke-TestCase -Name "monitor-$behavior" -Behavior $behavior -ChangeAt 2 -Drained $true
        Assert-Test ($test.Elapsed -lt 7000) "$behavior bounded without heartbeat"
        $receipt = Read-BureauJson ([IO.File]::ReadAllText($test.Receipt))
        Assert-Test ($receipt.StopSent -and $receipt.ExitCode -eq 1) "$behavior native refusal remains failure after stopped"
    }
    foreach ($change in @(@{ MemoryFreeBytes = [long] 1 }, @{ RegistrationStamp = 'aaaaaaaaaaaaaaaa' },
        @{ FileId = ('b' * 32) })) {
        $name = 'changed-' + @($change.Keys)[0]
        $null = Invoke-TestCase -Name $name -Mutation $change -ChangeAt 2 -Drained $true
    }
    $null = Invoke-TestCase -Name 'shutdown-identity' -Fixture 'shutdown-identity' -Behavior 'throw' -ChangeAt 3 -Beats 1
    $null = Invoke-TestCase -Name 'shutdown-registration' -Mutation @{ RegistrationStamp = 'aaaaaaaaaaaaaaaa' } -ChangeAt 3 -Beats 1 -Drained $true
    $final = Invoke-TestCase -Name 'shutdown-frozen-probe' -Behavior 'freeze' -ChangeAt 3 -Beats 1 -Drained $true
    Assert-Test (!$final.Probe.Disposed -and $final.Elapsed -lt 7000) 'pending final probe cannot block disposal'
}

function Test-ConfigAndPins {
    $examplePath = [IO.Path]::Combine($script:Windows, 'config.example.json')
    $example = Read-BureauJson ([IO.File]::ReadAllText($examplePath))
    Assert-Test (!$example.approved -and !$example.qualified) 'example cannot authorize execution'
    $refused = $false
    try { $null = Read-BureauBundle $examplePath } catch { $refused = $true }
    Assert-Test $refused 'default entry refuses before source loading'
    $cloudUser = New-TestConfig
    $cloudUser.windowsUserSid = 'S-1-12-1-111111111-222222222-333333333-444444444'
    Assert-BureauConfig $cloudUser
    $script:Checks++
    foreach ($field in @('approved', 'qualified')) {
        $config = New-TestConfig
        $config[$field] = $false
        $effects = New-TestEffects (New-TestProbe $config) 'single' ''
        $result = Invoke-BureauSupervisor $config $effects $script:NativeBudget
        Assert-Test (!$result.Spawned -and !$result.Success -and $effects.Record.Starts -eq 0) "explicit $field"
    }
    $config = New-TestConfig
    $config.schema = @('bureau-windows-supervision-v1')
    $result = Invoke-BureauSupervisor $config (New-TestEffects (New-TestProbe $config) 'single' '') $script:NativeBudget
    Assert-Test (!$result.Spawned -and !$result.Success) 'configuration schema is a string'
    foreach ($limits in @(@{ vhdBytesMax = [long] 15032385537 }, @{ backingFreeBytesMin = [long] 8589934591 },
        @{ memoryFreeBytesMin = [long] 1073741823 })) {
        $config = New-TestConfig
        foreach ($key in $limits.Keys) { $config.limits[$key] = $limits[$key] }
        $result = Invoke-BureauSupervisor $config (New-TestEffects (New-TestProbe $config) 'single' '') $script:NativeBudget
        Assert-Test (!$result.Spawned -and !$result.Success) 'caps cannot loosen'
    }
    $config = New-TestConfig
    $config.sourceSha256['protocol.ps1'] = ('0' * 64)
    $path = [IO.Path]::Combine($script:Scratch, 'bad-pin.json')
    [IO.File]::WriteAllText($path, ($config | ConvertTo-Json -Depth 8))
    $refused = $false
    try { $null = Read-BureauBundle $path } catch { $refused = $true }
    Assert-Test $refused 'unreviewed source refused before loading'
    $refused = $false
    try { $null = Read-BureauJson '{"approved":true,"approved":false}' } catch { $refused = $true }
    Assert-Test $refused 'duplicate config keys refused'
    Assert-Test ((15032385536 - 14877196288) -eq 155189248) 'small supplied margin does not replace startup reserve'
    $refused = $false
    try {
        $write = [IO.FileStream]::new($script:ConfigPath, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        $write.Dispose()
    } catch { $refused = $true }
    Assert-Test $refused 'reviewed configuration remains write-locked'
}

function Test-EnvironmentAndArguments {
    $names = @('GH_TOKEN', 'GITHUB_TOKEN', 'COPILOT_GITHUB_TOKEN', 'SSH_AUTH_SOCK', 'HTTP_PROXY',
        'HTTPS_PROXY', 'ALL_PROXY', 'RUSTC', 'RUSTC_WRAPPER', 'CARGO_TARGET_DIR', 'BASH_ENV',
        'WSLENV', 'WSL_INTEROP', 'COMSPEC', 'DOTNET_STARTUP_HOOKS', 'PATH')
    $saved = @{}
    try {
        foreach ($name in $names) {
            $saved[$name] = [Environment]::GetEnvironmentVariable($name)
            [Environment]::SetEnvironmentVariable($name, 'offline-inherited-must-not-escape')
        }
        $test = Invoke-TestCase -Name 'environment' -Success $true -Beats 1 -Drained $true
        $info = $test.Effects.Record.Info
        $expected = @('--distribution', 'BureauOperations', '--user', 'root', '--exec', '/usr/bin/env',
            '-i', 'PATH=/usr/bin:/bin', '/bin/bash', '/opt/bureau/source/deployment/windows-entry.sh', $script:Commit)
        Assert-Test ([string]::Join('|', $info.ArgumentList) -ceq [string]::Join('|', $expected)) 'exact argument list'
        Assert-Test ($info.Environment.Count -eq 3 -and $info.Environment['PATH'] -ceq [Environment]::SystemDirectory) 'explicit environment'
        Assert-Test ($info.StandardInputEncoding.CodePage -eq 65001 -and
            $info.StandardInputEncoding.GetPreamble().Length -eq 0) 'strict UTF8 input without BOM'
        Assert-Test ($info.FileName -ceq [IO.Path]::Combine([Environment]::SystemDirectory, 'wsl.exe') -and
            !$info.UseShellExecute -and $info.RedirectStandardInput -and $info.RedirectStandardOutput -and
            $info.RedirectStandardError) 'trusted executable and attached pipes'
    } finally {
        foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
    }
}

function Test-OwnerOverlap {
    $holder = [Diagnostics.Process]::Start((New-TestProcessInfo @($script:TestFile, '-Mode', 'mutex')))
    try {
        $ready = $holder.StandardOutput.ReadLineAsync()
        Assert-Test ($ready.Wait(5000) -and $ready.Result -ceq 'locked') 'fixture owns global mutex'
        $config = New-TestConfig
        $probe = New-TestProbe $config
        $effects = New-TestEffects $probe 'single' ''
        $result = Invoke-BureauSupervisor $config $effects $script:NativeBudget
        Assert-Test (!$result.Success -and !$result.Spawned -and $probe.Count -eq 0 -and
            $effects.Record.Starts -eq 0) 'overlapping owner refused before probe or spawn'
    } finally {
        $holder.StandardInput.Close()
        if (!$holder.WaitForExit(5000)) { $holder.Kill(); $null = $holder.WaitForExit(1000) }
        $holder.Dispose()
    }
}

function Test-TaskPreparation {
    $scriptPath = [IO.Path]::Combine($script:Windows, 'prepare-task.ps1')
    $supervisor = [IO.Path]::Combine($script:Windows, 'supervise.ps1')
    $target = [IO.Path]::Combine($script:Scratch, 'disabled-task.xml')
    $arguments = @($scriptPath, '-UserSid', $script:Config.windowsUserSid, '-PwshPath', $script:Pwsh,
        '-PwshSha256', (Get-FileHash -LiteralPath $script:Pwsh -Algorithm SHA256).Hash.ToLowerInvariant(),
        '-SupervisorPath', $supervisor, '-SupervisorSha256', $script:Config.sourceSha256['supervise.ps1'],
        '-ConfigPath', $script:ConfigPath, '-ConfigSha256',
        (Get-FileHash -LiteralPath $script:ConfigPath -Algorithm SHA256).Hash.ToLowerInvariant(), '-OutputPath', $target)
    $process = [Diagnostics.Process]::Start((New-TestProcessInfo $arguments))
    try {
        Assert-Test ($process.WaitForExit(10000) -and $process.ExitCode -eq 0) 'preparation exits without task activity'
    } finally {
        if (!$process.HasExited) { $process.Kill(); $null = $process.WaitForExit(1000) }
        $process.Dispose()
    }
    $xml = [xml] [IO.File]::ReadAllText($target)
    $wrongNamespace = @($xml.SelectNodes('//*') | Where-Object { $_.NamespaceURI -cne 'http://schemas.microsoft.com/windows/2004/02/mit/task' })
    Assert-Test ($wrongNamespace.Count -eq 0) 'task elements use the scheduler XML schema'
    Assert-Test ($xml.Task.Settings.Enabled -ceq 'false' -and $xml.Task.Triggers.LogonTrigger.Enabled -ceq 'false' -and
        $xml.Task.Principals.Principal.LogonType -ceq 'InteractiveToken') 'disabled interactive logon only'
    Assert-Test ($xml.Task.Principals.Principal.UserId -ceq $script:Config.windowsUserSid -and
        $xml.Task.Actions.Exec.Command -ceq $script:Pwsh -and
        $xml.Task.Actions.Exec.Arguments -ceq "-NoProfile -NonInteractive -File `"$supervisor`" -ConfigPath `"$script:ConfigPath`"") 'pinned paths without credential arguments'
    $escapedSupervisor = [IO.Path]::Combine($script:Scratch, 'supervisor & space.ps1')
    $escapedConfig = [IO.Path]::Combine($script:Scratch, 'config & space.json')
    $escaped = [xml] (New-BureauLogonTaskXml $script:Config.windowsUserSid $script:Pwsh $escapedSupervisor $escapedConfig)
    Assert-Test ($escaped.Task.Actions.Exec.Arguments.Contains('config & space.json"')) 'task XML escaping'
    $refused = $false
    try { Assert-BureauTaskUser $script:ConfigPath 'S-1-5-21-1-2-3-1001' } catch { $refused = $true }
    Assert-Test $refused 'task user must match pinned configuration'
}

function Invoke-TestWorker {
    $script:Scratch = [IO.Path]::Combine($script:Windows, ".offline-test-$PID-$([guid]::NewGuid().ToString('N'))")
    $null = [IO.Directory]::CreateDirectory($script:Scratch)
    $bundle = $null
    try {
        . ([IO.Path]::Combine($script:Windows, 'supervise.ps1'))
        $script:Config = New-TestConfig
        $script:ConfigPath = [IO.Path]::Combine($script:Scratch, 'approved & reviewed.json')
        [IO.File]::WriteAllText($script:ConfigPath, ($script:Config | ConvertTo-Json -Depth 8))
        $bundle = Read-BureauBundle $script:ConfigPath
        $script:NativeBudget = $bundle.NativeBudget
        foreach ($name in @('policy.ps1', 'protocol.ps1', 'process.ps1', 'supervisor.ps1', 'prepare-task.ps1')) {
            . ([scriptblock]::Create($bundle.Texts[$name]))
        }
        Add-Type -TypeDefinition ($bundle.Texts['HostNative.cs'] + "`n" + $bundle.Texts['HostProbe.cs'])
        Test-ResourceLayouts
        Test-ExactProtocolText
        Test-ExactConfigText
        Test-ExactResourceText
        Test-ExactHashAndTaskText
        Test-ConfigAndPins
        Test-PreSpawnPolicy
        Test-StartupReserve
        Test-StartupMemory
        Test-DerivedNativeMemory
        Test-NativeBudgetValidation
        Test-OwnerOverlap
        Test-EnvironmentAndArguments
        $first = Invoke-TestCase -Name 'healthy' -Fixture 'healthy' -Success $true -Beats 3 -Drained $true
        $starting = Invoke-TestCase -Name 'continued-starting' -Fixture 'continued-starting' -Success $true -Beats 4 -Drained $true
        Assert-Test ($starting.Probe.Count -eq 6) 'fresh guards continue throughout starting and running'
        $second = Invoke-TestCase -Name 'late-initial' -Fixture 'late-initial' -Success $true -Beats 1 -Drained $true
        $null = Invoke-TestCase -Name 'frame-boundary' -Fixture 'frame-boundary' -Success $true -Beats 1 -Drained $true
        Assert-Test ((Read-BureauJson ([IO.File]::ReadAllText($first.Receipt))).Owner -cne
            (Read-BureauJson ([IO.File]::ReadAllText($second.Receipt))).Owner) 'new random owner each lifetime'
        Assert-Test ($first.Probe.Count -eq 5) 'fresh probe per challenge plus admission and drain'
        Test-ProtocolFailures
        Test-DrainEvidence
        Test-NativeDrainProducer
        Test-MonitorFailures
        $initial = Invoke-TestCase -Name 'no-challenge' -Fixture 'no-challenge'
        Assert-Test ($initial.Elapsed -ge 20000 -and $initial.Elapsed -lt 24000) 'initial deadline bounded at 20 seconds'
        $stall = Invoke-TestCase -Name 'stall-after-challenge' -Fixture 'stall-after-challenge' -Beats 1 -Drained $true
        Assert-Test ($stall.Elapsed -ge 6000 -and $stall.Elapsed -lt 10000) 'later receive deadline bounded'
        $drain = Invoke-TestCase -Name 'drain-stall' -Fixture 'drain-stall' -Beats 1
        Assert-Test ($drain.Elapsed -ge 25000 -and $drain.Elapsed -lt 29000) 'unconfirmed drain waits 25 seconds then bounds client'
        $loss = Invoke-TestCase -Name 'loss-after-admission' -Fixture 'loss-after-admission' -Beats 2
        Assert-Test ($loss.Elapsed -ge 25000 -and $loss.Elapsed -lt 29000) 'lost client retains owner through drain deadline'
        Test-TaskPreparation
        [Console]::WriteLine("PASS $script:Checks offline assertions; no WSL, native unit, network, credential, or task registration calls.")
    } finally {
        if ($null -ne $bundle) { foreach ($file in $bundle.Locks) { $file.Dispose() } }
        [IO.Directory]::Delete($script:Scratch, $true)
    }
}

function Invoke-BoundedTestWorker {
    $node = (Get-Command node -CommandType Application -ErrorAction Stop).Source
    $process = [Diagnostics.Process]::Start((New-TestProcessInfo @($script:TestFile, '-Mode', 'worker', '-NodePath', $node)))
    $output = $process.StandardOutput.ReadToEndAsync()
    $errors = $process.StandardError.ReadToEndAsync()
    try {
        if (!$process.WaitForExit(300000)) { $process.Kill($true); throw 'offline suite exceeded 300 seconds' }
        [Console]::Write($output.GetAwaiter().GetResult())
        [Console]::Error.Write($errors.GetAwaiter().GetResult())
        return $process.ExitCode
    } finally {
        if (!$process.HasExited) { $process.Kill($true); $null = $process.WaitForExit(1000) }
        $process.Dispose()
    }
}

switch ($Mode) {
    'client' { Invoke-TestClient }
    'mutex' { Invoke-TestMutexHolder }
    'worker' { Invoke-TestWorker }
    'run' { exit (Invoke-BoundedTestWorker) }
    default { throw 'unknown-offline-test-mode' }
}
