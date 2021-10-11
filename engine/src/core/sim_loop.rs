use log::info;
use super::frame::Frame;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The core simulation loop within the engine. This loop will work as fast as it can to run simulation frames in both open and fixed frames.
/// The open will simply run as fast is it can only gated by the actual processing work being done in the update logic. The fixed updates 
/// will occur on a set fixed time step and can be used for time sensitive logic like physics.
pub struct SimLoop {}

impl SimLoop {
  /// Start the simulation loop at a fixed time step. The caller must provide the seed simulation data to be manipulated by the updates 
  /// routines. The caller must supply the update and fixed update routines to invoke throughout the simulation.
  pub fn start<Data, UF, FUF>(fixed_time_step_in_nano: u64, data: Data, on_update: UF, on_fixed_update: FUF, stop_handle: Arc<AtomicBool>)
  where
    UF: Fn(&Frame, &mut Data) -> () + Send + Sync,
    FUF: Fn(&Frame, &mut Data) -> () + Send + Sync,
  {
    let mut frame = Frame::new(fixed_time_step_in_nano);
    let mut accumulator: u64 = 0;
    let mut data = data;
    loop {
      frame = frame.next();
      accumulator += frame.delta.as_nanos() as u64;
      while accumulator >= fixed_time_step_in_nano {
        frame.increment_fixed();
        on_fixed_update(&frame, &mut data);
        accumulator -= fixed_time_step_in_nano;
      }
      // println!("Ac: {}", accumulator);
      on_update(&frame, &mut data);
      if stop_handle.load(Ordering::Relaxed) {
        info!("Ending simulation due to stop handle");
        return;
      }
      std::thread::sleep(std::time::Duration::from_millis(1));
    }
  }
}
