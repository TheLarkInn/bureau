function New-BureauOwner {
    $mutex = [Threading.Mutex]::new($false, 'Global\BureauMaintenanceOwner')
    try {
        try { $owned = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] {
            $mutex.ReleaseMutex()
            throw 'previous-owner-lost'
        }
        if (!$owned) { throw 'owner-already-active' }
        return $mutex
    } catch { $mutex.Dispose(); throw 'machine-owner-refused' }
}

function New-BureauStartInfo {
    param($Config)
    $system = [Environment]::SystemDirectory
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = [IO.Path]::Combine($system, 'wsl.exe')
    $info.WorkingDirectory = $system
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardInputEncoding = [Text.UTF8Encoding]::new($false, $true)
    $info.StandardOutputEncoding = [Text.UTF8Encoding]::new($false, $true)
    $info.StandardErrorEncoding = [Text.UTF8Encoding]::new($false, $true)
    $info.Environment.Clear()
    $info.Environment['SystemRoot'] = [IO.Path]::GetDirectoryName($system)
    $info.Environment['WINDIR'] = [IO.Path]::GetDirectoryName($system)
    $info.Environment['PATH'] = $system
    foreach ($argument in @('--distribution', $Config.distro, '--user', 'root', '--exec',
        '/usr/bin/env', '-i', 'PATH=/usr/bin:/bin', '/bin/bash',
        '/opt/bureau/source/deployment/windows-entry.sh', $Config.commit)) {
        $info.ArgumentList.Add($argument)
    }
    return $info
}

function New-BureauEffects {
    return @{
        NewProbe = { param($config)
            [Bureau.Windows.HostProbe]::new($config.distro, $config.registrationId, $config.vhdPath) }
        StartClient = { param($info)
            Assert-BureauRegularPath $info.FileName
            [Diagnostics.Process]::Start($info) }
        StopRequested = { [Bureau.Windows.Interrupt]::Requested }
        NewDrainClock = { [Diagnostics.Stopwatch]::StartNew() }
    }
}

function Assert-BureauActive {
    param($Context, [switch] $NoAhead)
    if (& $Context.Effects.StopRequested) { throw 'windows-interrupted' }
    if ($null -eq $Context.Channel) { return }
    Update-BureauChannel $Context.Channel
    if ($null -ne $Context.Channel.Fault -or $Context.Channel.Eof) { throw 'native-stream-failed' }
    if ($NoAhead -and ($null -ne $Context.Channel.Frame -or $Context.Channel.Buffer.Length -gt 0)) {
        throw 'native-output-before-heartbeat'
    }
}

function Wait-BureauTask {
    param($Task, $Context, [Diagnostics.Stopwatch] $Watch, [long] $Limit, [switch] $NoAhead)
    do {
        if ($Watch.Elapsed.TotalMilliseconds -gt $Limit) { throw 'operation-deadline' }
        if ($Task.IsCompleted) { return $Task.GetAwaiter().GetResult() }
        Assert-BureauActive $Context -NoAhead:$NoAhead
        [Threading.Thread]::Sleep(5)
    } while ($true)
}

function Get-BureauFreshSample {
    param($Context, [switch] $NoAhead)
    $started = [Diagnostics.Stopwatch]::GetTimestamp()
    $Context.SampleWatch = [Diagnostics.Stopwatch]::StartNew()
    $Context.SampleTask = $Context.Probe.SampleAsync()
    $sample = Wait-BureauTask $Context.SampleTask $Context $Context.SampleWatch 2000 -NoAhead:$NoAhead
    Assert-BureauSample $sample $Context.Config $started $Context.PreviousSample
    $Context.PreviousSample = $sample
}

function Receive-BureauFrame {
    param($Context, [long] $Limit)
    $watch = [Diagnostics.Stopwatch]::StartNew()
    do {
        Update-BureauChannel $Context.Channel
        if ($Context.Channel.Fault) { throw 'native-stream-failed' }
        if ($null -ne $Context.Channel.Frame) {
            $text = $Context.Channel.Frame
            $Context.Channel.Frame = $null
            try { return Read-BureauFrame $text $Context.Protocol } catch {
                $Context.ProtocolFault = $true
                throw 'native-frame-refused'
            }
        }
        if ($Context.Channel.Eof -or $watch.ElapsedMilliseconds -gt $Limit -or
            (& $Context.Effects.StopRequested)) { throw 'native-receive-failed' }
        [Threading.Thread]::Sleep(5)
    } while ($true)
}

function Send-BureauHeartbeat {
    param($Context, $Frame)
    Get-BureauFreshSample $Context -NoAhead
    if (!$Context.InitialHeartbeatSent) {
        Assert-BureauStartupReserve $Context.PreviousSample $Context.Config.limits
        Assert-BureauStartupMemory $Context.PreviousSample $Context.NativeBudget $Context.Config.limits
    }
    Assert-BureauActive $Context -NoAhead
    $bytes = New-BureauHeartbeat $Frame $Context.Owner $Context.Config.commit $Context.SampleWatch.ElapsedMilliseconds
    $stream = $Context.Process.StandardInput.BaseStream
    Assert-BureauActive $Context -NoAhead
    if ($Context.SampleWatch.Elapsed.TotalMilliseconds -gt 2000) { throw 'stale-sample-before-write' }
    $write = $stream.WriteAsync($bytes, 0, $bytes.Length)
    $null = Wait-BureauTask $write $Context $Context.SampleWatch 2000
    $null = Wait-BureauTask ($stream.FlushAsync()) $Context $Context.SampleWatch 2000
    $Context.InitialHeartbeatSent = $true
}
