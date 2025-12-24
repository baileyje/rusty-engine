use crossbeam::channel::{unbounded, Receiver, Sender};
use log::{Level, Metadata, Record};

#[derive(Debug)]
pub struct LogMessage {
    pub level: Level,
    pub message: String,
}

pub struct ChannelLogger {
    sender: Sender<LogMessage>,
}

impl log::Log for ChannelLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let _ = self.sender.try_send(LogMessage {
                level: record.metadata().level(),
                message: format!("{}", record.args()),
            });
        }
    }

    fn flush(&self) {}
}

impl ChannelLogger {
    pub fn new(sender: Sender<LogMessage>) -> Self {
        Self { sender }
    }

    pub fn with_receiver() -> (Self, Receiver<LogMessage>) {
        let (sender, receiver) = unbounded();
        (Self::new(sender), receiver)
    }
}
