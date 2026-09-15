use serde_json::Value;
use tokio::sync::mpsc;

use super::{CallbackRequest, Command, Incoming, Input};

const fn kind(input: &Input) -> &'static str {
    match input {
        Input::Frame(Some(Ok(_))) => "frame",
        Input::Command(Some(_)) => "command",
        _ => panic!("expected a queued input"),
    }
}

fn command() -> Command {
    Command::callback(CallbackRequest {
        method: "ping".to_owned(),
        params: Value::Null,
    })
}

fn frames() -> mpsc::Receiver<Incoming> {
    let (sender, receiver) = mpsc::channel(2);
    sender.try_send(Ok(Value::Null)).expect("first frame");
    sender.try_send(Ok(Value::Null)).expect("second frame");
    receiver
}

fn commands() -> mpsc::Receiver<Command> {
    let (sender, receiver) = mpsc::channel(2);
    sender.try_send(command()).expect("first command");
    sender.try_send(command()).expect("second command");
    receiver
}

#[tokio::test]
async fn ready_channels_preserve_frames_and_commands_in_alternating_order() {
    let mut incoming = frames();
    let mut outgoing = commands();
    let mut selected = Vec::new();
    for incoming_first in [true, false, true, false] {
        selected.push(kind(
            &Input::receive(&mut incoming, &mut outgoing, incoming_first).await,
        ));
    }
    assert_eq!(selected, ["frame", "command", "frame", "command"]);
}

#[tokio::test]
async fn an_idle_preferred_channel_does_not_block_the_ready_channel() {
    let (_sender, mut incoming) = mpsc::channel(2);
    let mut outgoing = commands();
    let input = Input::receive(&mut incoming, &mut outgoing, true).await;
    assert_eq!(kind(&input), "command");
}

#[tokio::test]
async fn a_closed_reader_is_preserved_as_an_input() {
    let (sender, mut incoming) = mpsc::channel(2);
    let mut outgoing = commands();
    drop(sender);
    let input = Input::receive(&mut incoming, &mut outgoing, true).await;
    assert!(matches!(input, Input::Frame(None)));
}
