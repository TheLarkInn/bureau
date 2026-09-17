use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

use anyhow::Context;

fn signals_caught(status: &str) -> anyhow::Result<bool> {
    let caught = status
        .lines()
        .find_map(|line| line.strip_prefix("SigCgt:"))
        .context("process status has no caught-signal mask")?;
    let mask = u64::from_str_radix(caught.trim(), 16)?;
    let required = (1 << 1) | (1 << 14);
    Ok(mask & required == required)
}

fn ready(pid: u32) -> anyhow::Result<bool> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status"))?;
    // SIGINT's listener is constructed before SIGTERM's handler is installed.
    signals_caught(&status)
}

fn wait_until(
    label: &str,
    mut condition: impl FnMut() -> anyhow::Result<bool>,
) -> anyhow::Result<()> {
    let started = Instant::now();
    while !condition()? {
        anyhow::ensure!(
            started.elapsed() < Duration::from_secs(10),
            "timed out waiting for {label}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

fn send_interrupt(pid: u32) -> anyhow::Result<()> {
    let status = Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()?;
    anyhow::ensure!(status.success(), "sending SIGINT failed: {status}");
    Ok(())
}

fn stop_if_running(child: &mut Child) -> anyhow::Result<()> {
    if child.try_wait()?.is_none() {
        child.kill()?;
    }
    Ok(())
}

pub fn interrupt(mut child: Child) -> anyhow::Result<Output> {
    let stopped = wait_until("signal handlers", || ready(child.id()))
        .and_then(|()| send_interrupt(child.id()))
        .and_then(|()| wait_until("daemon shutdown", || Ok(child.try_wait()?.is_some())));
    if stopped.is_err() {
        stop_if_running(&mut child)?;
    }
    let output = child.wait_with_output()?;
    stopped.with_context(|| format!("reconcile interruption failed: {output:?}"))?;
    Ok(output)
}

#[test]
fn readiness_requires_both_caught_signals() {
    for (mask, expected) in [("0", false), ("2", false), ("4000", false), ("4002", true)] {
        assert_eq!(
            signals_caught(&format!("Name:\tbureau\nSigCgt:\t{mask}\n")).expect("valid status"),
            expected
        );
    }
}

#[test]
fn missing_or_malformed_signal_masks_are_errors() {
    for status in ["Name:\tbureau\n", "SigCgt:\tinvalid\n"] {
        assert!(signals_caught(status).is_err());
    }
}
