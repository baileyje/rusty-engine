use std::sync::mpsc::{channel, Receiver, Sender};

pub trait Logger {
  fn debug(&self, message: String);
  fn info(&self, message: String);
  fn warn(&self, message: String);
  fn error(&self, message: String);
}

pub struct JankyLogger {}

impl Logger for JankyLogger {
  fn debug(&self, message: String) {
    println!("DEBUG - {}", message);
  }

  fn info(&self, message: String) {
    println!("INFO - {}", message);
  }

  fn warn(&self, message: String) {
    println!("WARN - {}", message);
  }

  fn error(&self, message: String) {
    println!("ERROR - {}", message);
  }
}

#[derive(Debug)]
pub struct LogMessage {
  pub level: String,
  pub message: String
}

pub struct ChannelLogger {
  sender: Sender<LogMessage>,
}

impl ChannelLogger {
  pub fn new() -> (Self, Receiver<LogMessage>) {
    let (sender, receiver) = channel();
    return (Self { sender }, receiver);
  }
}

impl Logger for ChannelLogger {
  fn debug(&self, message: String) {
    self.sender.send(LogMessage{ level: "DEBUG".into(), message} ).unwrap();
  }

  fn info(&self, message: String) {
    self.sender.send(LogMessage{ level: "INFO".into(), message} ).unwrap();
  }

  fn warn(&self, message: String) {
    self.sender.send(LogMessage{ level: "WARN".into(), message} ).unwrap();
  }

  fn error(&self, message: String) {
    self.sender.send(LogMessage{ level: "ERROR".into(), message} ).unwrap();
  }
}

/// Initialize the logger for the engine.
pub fn init() {
  // let _ = env_logger::try_init();
}
