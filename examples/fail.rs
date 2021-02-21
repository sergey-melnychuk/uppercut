use std::sync::mpsc::{channel, Sender};

extern crate uppercut;
use uppercut::api::{AnyActor, Envelope, AnySender};
use uppercut::core::System;
use uppercut::pool::ThreadPool;
use std::any::Any;
use std::thread::sleep;
use std::time::Duration;

struct Message(Option<usize>, Sender<usize>);

#[derive(Default)]
struct State;

impl AnyActor for State {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(msg) = envelope.message.downcast_ref::<Message>() {
            let x = msg.0.unwrap();
            msg.1.send(x).unwrap();
            sender.log(&format!("Actor 'State' received a message: {}", x));
        }
    }

    fn on_fail(&self, _error: Box<dyn Any + Send>, sender: &mut dyn AnySender) {
        sender.log("failure detected!");
    }

    fn on_stop(&self, sender: &mut dyn AnySender) {
        sender.log("shutting down");
    }
}

fn main() {
    let sys = System::default();
    let pool = ThreadPool::new(6);
    let run = sys.run(&pool).unwrap();
    run.spawn_default::<State>("x");

    let (tx, rx) = channel();
    run.send("x", Envelope::of(Message(Some(42), tx.clone())));
    run.send("x", Envelope::of(Message(None, tx.clone())));
    run.send("x", Envelope::of(Message(Some(100500), tx.clone())));

    println!("recv: {}", rx.recv().unwrap());
    sleep(Duration::from_secs(3));
    run.shutdown();
}
