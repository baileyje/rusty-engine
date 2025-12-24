use crate::core::{runner::RunResult, Engine};

pub fn once(engine: &mut Engine) -> RunResult {
    engine.update();
    RunResult::Success
}
