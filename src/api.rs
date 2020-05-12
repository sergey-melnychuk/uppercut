use std::any::Any;
use std::time::Duration;

pub type Actor = Box<dyn AnyActor + Send>;

pub trait AnyActor {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender);
}

pub trait AnySender {
    fn me(&self) -> &str;
    fn myself(&self) -> String;
    fn send(&mut self, address: &str, message: Envelope);
    fn spawn(&mut self, address: &str, f: fn() -> Actor);
    fn delay(&mut self, address: &str, envelope: Envelope, duration: Duration);
    fn stop(&mut self, address: &str);
}

#[derive(Debug)]
pub struct Envelope {
    pub message: Box<dyn Any + Send>,
    pub from: String,
}

impl Default for Envelope {
    fn default() -> Self {
        Envelope {
            message: Box::new(()),
            from: String::default(),
        }
    }
}

impl Envelope {
    pub fn of<T: Any + Send>(message: T, from: &str) -> Envelope {
        Envelope {
            message: Box::new(message),
            from: from.to_string(),
        }
    }
}
