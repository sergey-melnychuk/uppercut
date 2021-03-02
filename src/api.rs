use std::any::Any;
use std::time::{Duration, SystemTime};
use std::fmt::Debug;

pub type Actor = Box<dyn AnyActor>;

pub type Message = Box<dyn Any + Send>;

pub trait AnyActor: Send {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender);
    fn on_fail(&self, _error: Box<dyn Any + Send>, _sender: &mut dyn AnySender) {}
    fn on_stop(&self, _sender: &mut dyn AnySender) {}
}

pub trait AnySender {
    fn me(&self) -> &str;
    fn myself(&self) -> String;
    fn send(&mut self, address: &str, envelope: Envelope);
    fn spawn(&mut self, address: &str, f: fn() -> Actor);
    fn delay(&mut self, address: &str, envelope: Envelope, duration: Duration);
    fn stop(&mut self, address: &str);
    fn log(&mut self, message: &str);
    fn metric(&mut self, name: &str, value: f64);
    fn now(&self) -> SystemTime;
}

#[derive(Debug)]
pub struct Envelope {
    pub message: Message,
    pub from: String,
    pub to: String,
}

impl Envelope {
    pub fn of<T: Any + Send + Debug>(message: T) -> Envelope {
        Envelope {
            message: Box::new(message),
            from: String::default(),
            to: String::default(),
        }
    }

    pub fn from(mut self, from: &str) -> Self {
        self.from = from.to_string();
        self
    }

    pub fn to(mut self, to: &str) -> Self {
        self.to = to.to_string();
        self
    }
}
