use std::sync::mpsc::{channel, Receiver, Sender};

#[derive(Debug)]
pub enum Level {
  Debug,
  Info,
  Warn,
  Error,
}

impl Level {
  fn string(&self) -> String {
    match self {
      Level::Debug => "DEBUG",
      Level::Info => "INFO",
      Level::Warn => "WARN",
      Level::Error => "ERROR",
    }
    .into()
  }
}

pub trait Logger {
  
  fn debug(&self, message: String) {
    self.log(LogMessage {
      level: Level::Debug,
      message,
    });
  }

  fn info(&self, message: String) {
    self.log(LogMessage {
      level: Level::Info,
      message,
    });
  }

  fn warn(&self, message: String) {
    self.log(LogMessage {
      level: Level::Warn,
      message,
    });
  }

  fn error(&self, message: String) {
    self.log(LogMessage {
      level: Level::Error,
      message,
    });
  }

  fn log(&self, message: LogMessage);
}

pub struct JankyLogger {}

impl Logger for JankyLogger {
  fn log(&self, message: LogMessage) {
    println!("{} - {}", message.level.string(), message.message);
  }

}

#[derive(Debug)]
pub struct LogMessage {
  pub level: Level,
  pub message: String,
}

pub struct ChannelLogger {
  sender: Sender<LogMessage>,
}

impl ChannelLogger {
  pub fn new(sender: Sender<LogMessage>) -> Self {
    return Self { sender };
  }

  pub fn with_receiver() -> (Self, Receiver<LogMessage>) {
    let (sender, receiver) = channel();
    return (Self::new(sender), receiver);
  }
}

impl Logger for ChannelLogger {
  fn log(&self, message: LogMessage) {
    self.sender.send(message).unwrap();
  }
}

/// Initialize the logger for the engine.
pub fn init() {
  // let _ = env_logger::try_init();
}
