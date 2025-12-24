use crate::core::{runner::RunResult, Engine};

pub fn looped(engine: &mut Engine) -> RunResult {
    loop {
        let should_continue = engine.update();
        if !should_continue {
            return RunResult::Success;
        }
    }
}
