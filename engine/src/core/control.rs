use log::{debug, info};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Temporary engine control mechanism. For now this will listen to stdin to for commands to feed the engine. Right now that is `stop`. 
/// Pretty fancy eh..
pub struct Control {}

impl Control {
  
  // Start the control system. Listen for commands until we see `stop`
  pub fn start(stop_handle: Arc<AtomicBool>) {
    loop {
      let mut command = String::new();
      io::stdin()
        .read_line(&mut command)
        .ok()
        .expect("Failed to read line");
      let command = command.trim();
      debug!("Control Command: {}", command);
      if command == "stop" {
        info!("Stop Requested");
        stop_handle.store(true, Ordering::Relaxed);
        return;
      }
    }
  }
}
