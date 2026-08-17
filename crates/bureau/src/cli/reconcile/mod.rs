//! `reconcile`: committed config refresh, recovery, and level-triggered work.

pub(super) mod active;
mod args;
mod build;
mod daemon;

use crate::cli::out;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use daemon::Daemon;

use args::ResolvedArgs;
pub use args::{Args, ForgeArg};

enum Cycle {
    Continue,
    Shutdown,
    Failed(anyhow::Error),
}

enum Startup {
    Exit,
    Continue(Box<Daemon>, active::Signals),
}

fn parse_interval(value: &str) -> anyhow::Result<Duration> {
    let (number, unit) = value
        .chars()
        .last()
        .filter(char::is_ascii_alphabetic)
        .map_or((value, 's'), |unit| (&value[..value.len() - 1], unit));
    let amount: u64 = number.parse()?;
    let seconds = match unit {
        's' => amount,
        'm' => amount.saturating_mul(60),
        'h' => amount.saturating_mul(3600),
        _ => anyhow::bail!("interval unit must be `s`, `m`, or `h`"),
    };
    anyhow::ensure!(seconds > 0, "interval must be positive");
    Ok(Duration::from_secs(seconds))
}

async fn pass_or_signal(
    daemon: &mut Daemon,
    signals: &mut active::Signals,
) -> anyhow::Result<bool> {
    let mut pass = Box::pin(daemon.pass());
    match active::until_signal(pass.as_mut(), signals).await {
        active::Until::Complete(result) => result.map(|()| true),
        active::Until::Signalled => Ok(false),
    }
}

async fn shutdown(daemon: Daemon, signals: &mut active::Signals) {
    out::line(format_args!(
        "reconcile is draining active runs [{}]; send a second signal to cancel them",
        daemon.active_ids().join(", ")
    ));
    daemon.drain(signals).await;
}

async fn after_first(daemon: Daemon, mut signals: active::Signals, once: bool) -> Startup {
    if once {
        daemon.drain(&mut signals).await;
        Startup::Exit
    } else {
        Startup::Continue(Box::new(daemon), signals)
    }
}

type StartupFuture = Pin<Box<dyn Future<Output = anyhow::Result<Startup>> + Send>>;

fn finish_startup(
    daemon: Daemon,
    signals: active::Signals,
    once: bool,
    passed: anyhow::Result<bool>,
) -> StartupFuture {
    match passed {
        Ok(true) => Box::pin(async move { Ok(after_first(daemon, signals, once).await) }),
        Ok(false) => Box::pin(async move {
            let mut signals = signals;
            shutdown(daemon, &mut signals).await;
            Ok(Startup::Exit)
        }),
        Err(error) => Box::pin(async move {
            let mut signals = signals;
            daemon.drain(&mut signals).await;
            Err(error)
        }),
    }
}

async fn startup(
    mut daemon: Daemon,
    mut signals: active::Signals,
    once: bool,
) -> anyhow::Result<Startup> {
    let passed = pass_or_signal(&mut daemon, &mut signals).await;
    finish_startup(daemon, signals, once, passed).await
}

async fn pass_cycle(daemon: &mut Daemon, signals: &mut active::Signals) -> Cycle {
    match pass_or_signal(daemon, signals).await {
        Ok(true) => Cycle::Continue,
        Ok(false) => Cycle::Shutdown,
        Err(error) => Cycle::Failed(error),
    }
}

async fn cycle(daemon: &mut Daemon, signals: &mut active::Signals, interval: Duration) -> Cycle {
    if active::wait_or_signal(interval, signals).await {
        return Cycle::Shutdown;
    }
    pass_cycle(daemon, signals).await
}

async fn continuous(
    mut daemon: Daemon,
    interval: Duration,
    mut signals: active::Signals,
) -> anyhow::Result<i32> {
    loop {
        match cycle(&mut daemon, &mut signals, interval).await {
            Cycle::Continue => {}
            Cycle::Shutdown => {
                shutdown(daemon, &mut signals).await;
                return Ok(0);
            }
            Cycle::Failed(error) => {
                out::error(format_args!("reconcile pass failed; retrying: {error}"));
            }
        }
    }
}

async fn continue_from(started: Startup, interval: Duration) -> anyhow::Result<i32> {
    match started {
        Startup::Exit => Ok(0),
        Startup::Continue(daemon, signals) => continuous(*daemon, interval, signals).await,
    }
}

pub(super) async fn run(args: Args) -> anyhow::Result<i32> {
    let args = args.resolve()?;
    let interval = parse_interval(&args.interval)?;
    let once = args.now;
    let daemon = daemon::new(&args)?;
    let signals = active::Signals::new()?;
    let started = startup(daemon, signals, once).await?;
    continue_from(started, interval).await
}

#[cfg(test)]
mod tests {
    use super::parse_interval;

    #[test]
    fn interval_accepts_seconds_minutes_and_hours() {
        let values = ["5", "5s", "2m", "3h"]
            .map(|value| parse_interval(value).expect("valid interval").as_secs());
        assert_eq!(values, [5, 5, 120, 10_800]);
    }

    #[test]
    fn interval_rejects_zero_and_unknown_units() {
        let invalid = ["0", "12d"]
            .into_iter()
            .all(|value| parse_interval(value).is_err());
        assert!(invalid);
    }
}
