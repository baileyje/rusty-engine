use std::sync::mpsc::{channel, Sender};
use std::thread::{spawn, JoinHandle};

/// Managed thread used within the Engine. These threads represent
#[derive(Debug)]
pub enum ThreadCommand {
  Stop,
  Pause,
}

pub struct EngineThread {
  handle: Option<JoinHandle<()>>,
  /// A channel sender that allows outside threads to send commands to this thread.
  pub sender: Sender<ThreadCommand>,
}

impl EngineThread {
  /// Spawn a the thread with a provided work function. This behaves different than the std::thread impl is this work function
  /// can be called many times as the engine seems sees fit based on this thread applies to the engine.
  pub fn spawn<W: 'static>(mut work: W) -> Self
  where
    W: FnMut() -> () + Send,
  {
    let (sender, receiver) = channel::<ThreadCommand>();
    let instance = Self {
      handle: Some(spawn(move || loop {
        if let Ok(msg) = receiver.try_recv() {
          match msg {
            ThreadCommand::Stop => {
              println!("Received message: {:?}", msg);
              return;
            },
            ThreadCommand::Pause => {
              continue;
            }
          }
        }
        work();
      })),
      sender,
    };
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
