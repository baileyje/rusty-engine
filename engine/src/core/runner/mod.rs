use crate::core::engine::Engine;
mod looped;
mod once;

pub enum RunResult {
    Success,
    Failure, // Todo: error details..
}

pub type Runner = Box<dyn FnOnce(&mut Engine) -> RunResult>;

pub fn no_op(_: &mut Engine) -> RunResult {
    RunResult::Success
}

pub use looped::looped;
pub use once::once;
