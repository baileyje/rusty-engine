use log::info;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::frame::{Frame, TimeFrame};


/// The core simulation loop within the engine. This loop will work as fast as it can to run simulation frames in both open and fixed frames.
/// The open will simply run as fast is it can only gated by the actual processing work being done in the update logic. The fixed updates 
/// will occur on a set fixed time step and can be used for time sensitive logic like physics.
pub struct SimLoop {}

impl SimLoop {
  /// Start the simulation loop at a fixed time step. The caller must provide the seed simulation data to be manipulated by the updates 
  /// routines. The caller must supply the update and fixed update routines to invoke throughout the simulation.
  pub fn start<Data, UF, FUF>(fixed_time_step_in_nano: u64, data: &mut Data, on_update: UF, on_fixed_update: FUF, stop_handle: Arc<AtomicBool>)
  where
    UF: Fn(Frame<Data>) -> (),
    FUF: Fn(Frame<Data>) -> (),
    {
    let mut time_frame = TimeFrame::new(fixed_time_step_in_nano);
    let mut accumulator: u64 = 0;
    loop {
      time_frame = time_frame.next();
      accumulator += time_frame.delta.as_nanos() as u64;
      while accumulator >= fixed_time_step_in_nano {
        time_frame.increment_fixed();
        on_fixed_update(Frame::new(time_frame, data));
        accumulator -= fixed_time_step_in_nano;
      }
      on_update(Frame::new(time_frame,  data));
      if stop_handle.load(Ordering::Relaxed) {
        info!("Ending simulation due to stop handle");
        return;
      }
      std::thread::sleep(std::time::Duration::from_millis(1));
    }
  }
}
