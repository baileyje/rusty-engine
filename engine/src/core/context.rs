use crate::core::Time;

pub struct Context {
    pub time: Time,
}

impl Context {
    pub fn new(time: Time) -> Self {
        Self { time }
    }
}
