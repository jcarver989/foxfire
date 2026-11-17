use crate::command::HostCommandResult;
use crossterm::event::Event;
use foxfire_core::runtime::Input;
use std::time::Instant;

pub enum Message {
    Terminal(Event),
    Session(Box<Input>),
    Host(Box<HostCommandResult>),
    Tick(Instant),
}
