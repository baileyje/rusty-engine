use std::{
  sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{channel, Sender, Receiver},
  },
  thread::{spawn, JoinHandle},
};
use super::logger::{ChannelLogger, LogMessage};

/// Managed thread used within the Engine. These threads represent
#[derive(Debug)]
pub enum ThreadCommand {
  Stop,
  Pause,
  Unpause,
}

pub struct EngineThread {
  // Thread handle used to join the underlying thread impl.
  handle: Option<JoinHandle<()>>,
  /// A channel sender that allows outside threads to send commands to this thread.
  sender: Sender<ThreadCommand>,
  /// Channel receiver for logged data
  pub log_receiver: Receiver<LogMessage>

}

impl EngineThread {
  /// Spawn a the thread with a provided work function. This behaves different than the std::thread impl is this work function
  /// can be called many times as the engine seems sees fit based on this thread applies to the engine.
  pub fn spawn<W: 'static>(mut work: W,) -> Self
  where
    W: FnMut(&ChannelLogger) -> () + Send,
  {
    let (sender, receiver) = channel::<ThreadCommand>();
    let (log_sender, log_receiver) = channel::<LogMessage>();
    let mut instance = Self {
      handle: None,
      sender,
      log_receiver
    };
    let paused = AtomicBool::new(false);
    instance.handle = Some(spawn(move || loop {
      let logger = ChannelLogger::new(log_sender.clone());
      if let Ok(msg) = receiver.try_recv() {
        match msg {
          ThreadCommand::Stop => {
            return;
          }
          ThreadCommand::Pause => {
            paused.store(true, Ordering::Relaxed);
          }
          ThreadCommand::Unpause => {
            paused.store(false, Ordering::Relaxed);
          }
        }
      }
      if !paused.load(Ordering::Relaxed) {
        work(&logger);
      }
    }));
    instance
  }

  /// Send a command to the underlying thread.
  pub fn send(&self, cmd: ThreadCommand) {
    self.sender.send(cmd).unwrap();
  }

  /// Stop the thread.
  pub fn stop(&self) {
    self.send(ThreadCommand::Stop);
  }

  /// Pause the thread.
  pub fn pause(&self) {
    self.send(ThreadCommand::Pause);
  }

  /// Pause the thread.
  pub fn unpause(&self) {
    self.send(ThreadCommand::Unpause);
  }

  /// Join the calling thread to the underlying thread.
  pub fn join(&mut self) {
    self
      .handle
      .take()
      .expect("Unable to get thread handle")
      .join()
      .expect("Unable to join thread");
  }
}
