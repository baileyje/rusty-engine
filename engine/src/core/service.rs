use std::any::type_name;

use crate::core::Engine;

fn type_name_of<T>(_: T) -> &'static str {
    type_name::<T>()
}

pub trait Service: Send + Sync {
    fn name(&self) -> String {
        type_name_of(self).to_string()
    }

    fn start(&mut self, _engine: &mut Engine) -> Result<(), &str> {
        Ok(())
    }
    fn stop(&mut self, _engine: &mut Engine) -> Result<(), &str> {
        Ok(())
    }
}

pub(crate) struct NoOpService;
impl Service for NoOpService {}
