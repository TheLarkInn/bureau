function New-BureauContext {
    param($Config, $Effects, $NativeBudget)
    return @{ Config = $Config; Effects = $Effects; Protocol = (New-BureauProtocol);
        NativeBudget = $NativeBudget; InitialHeartbeatSent = $false;
        Owner = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(16)).ToLowerInvariant();
        Process = $null; Channel = $null; Probe = $null; PreviousSample = $null;
        SampleTask = $null; SampleWatch = $null; Failure = $false; ProtocolFault = $false;
        Drained = $false; DrainLimit = [long] 25000 }
}

function Invoke-BureauExchange {
    param($Context)
    Get-BureauFreshSample $Context
    Assert-BureauStartupReserve $Context.PreviousSample $Context.Config.limits
    Assert-BureauStartupMemory $Context.PreviousSample $Context.NativeBudget $Context.Config.limits
    $Context.Process = & $Context.Effects.StartClient (New-BureauStartInfo $Context.Config)
    if ($Context.Process -isnot [Diagnostics.Process]) { throw 'native-client-not-started' }
    $Context.Channel = New-BureauChannel $Context.Process
    $limit = 20000
    do {
        $frame = Receive-BureauFrame $Context $limit
        if ($frame.type -ceq 'stopped') { return }
        Send-BureauHeartbeat $Context $frame
        $limit = 6000
    } while ($true)
}

function Read-BureauDrainFrame {
    param($Context)
    Update-BureauChannel $Context.Channel
    if ($Context.Channel.Fault) { $Context.Failure = $true }
    if ($Context.Channel.OutputBad) { $Context.ProtocolFault = $true }
    if ($null -eq $Context.Channel.Frame) { return }
    $text = $Context.Channel.Frame
    $Context.Channel.Frame = $null
    try { $null = Read-BureauFrame $text $Context.Protocol } catch {
        $Context.ProtocolFault = $true
        $Context.Failure = $true
    }
}

function Test-BureauClientFinished {
    param($Context)
    if (!$Context.Process.HasExited) { return $false }
    if ($null -eq $Context.Channel) { return $true }
    return ($Context.Channel.Eof -or $Context.Channel.OutputBad) -and
        ($Context.Channel.ErrorEof -or $Context.Channel.ErrorBad)
}

function Test-BureauDrained {
    param($Context)
    return $null -ne $Context.Channel -and $Context.Protocol.Stopped -and !$Context.ProtocolFault -and
        $Context.Channel.Eof -and !$Context.Channel.OutputBad -and $Context.Channel.ErrorEof -and
        !$Context.Channel.ErrorBad -and $Context.Process.HasExited
}

function Confirm-BureauFinalSample {
    param($Context, $Watch)
    if ($Context.SampleTask -and !$Context.SampleTask.IsCompleted) { throw 'sample-still-pending' }
    $remaining = $Context.DrainLimit - $Watch.ElapsedMilliseconds
    if ($remaining -lt 2000) { throw 'final-sample-deadline' }
    $final = $Context.Clone()
    $final.Channel = $null
    $final.Effects = @{ StopRequested = { $false } }
    try { Get-BureauFreshSample $final } finally { $Context.SampleTask = $final.SampleTask }
}

function Close-BureauTransaction {
    param($Context)
    $watch = & $Context.Effects.NewDrainClock
    try { $Context.Process.StandardInput.Close() } catch { $Context.Failure = $true }
    do {
        if ($null -ne $Context.Channel) { Read-BureauDrainFrame $Context }
        if (Test-BureauClientFinished $Context) {
            if ($Context.Process.ExitCode -ne 0) { $Context.Failure = $true }
            $Context.Drained = Test-BureauDrained $Context
            if ($Context.Drained) {
                try { Confirm-BureauFinalSample $Context $watch } catch { $Context.Failure = $true }
                return
            }
        }
        [Threading.Thread]::Sleep(5)
    } while ($watch.ElapsedMilliseconds -lt $Context.DrainLimit)
    $Context.Failure = $true
    if (!$Context.Process.HasExited) {
        try { $Context.Process.Kill(); $null = $Context.Process.WaitForExit(1000) } catch { }
    }
}

function Dispose-BureauContext {
    param($Context)
    if ($null -ne $Context.Probe -and
        ($null -eq $Context.SampleTask -or $Context.SampleTask.IsCompleted)) { $Context.Probe.Dispose() }
    if ($null -ne $Context.Channel) {
        $Context.Channel.Buffer.Dispose()
        $Context.Channel.ErrorBuffer.Dispose()
    }
    if ($null -ne $Context.Process) { $Context.Process.Dispose() }
}

function Invoke-BureauSupervisor {
    param($Config, $Effects, $NativeBudget)
    $context = New-BureauContext $Config $Effects $NativeBudget
    $mutex = $null
    $interrupt = $null
    try {
        Assert-BureauConfig $Config
        Assert-BureauNativeBudget $NativeBudget
        $mutex = New-BureauOwner
        $interrupt = [Bureau.Windows.Interrupt]::new()
        $context.Probe = & $Effects.NewProbe $Config
        Invoke-BureauExchange $context
    } catch { $context.Failure = $true } finally {
        try {
            if ($null -ne $context.Process) { Close-BureauTransaction $context }
        } catch { $context.Failure = $true } finally {
            try { Dispose-BureauContext $context } finally {
                if ($null -ne $interrupt) { $interrupt.Dispose() }
                if ($null -ne $mutex) { $mutex.ReleaseMutex(); $mutex.Dispose() }
            }
        }
    }
    return @{ Success = (!$context.Failure -and $context.Drained); Drained = $context.Drained;
        Spawned = ($null -ne $context.Process) }
}
