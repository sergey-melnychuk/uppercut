use std::sync::mpsc::{channel, Sender};

extern crate uppercut;
use std::any::Any;
use std::thread::sleep;
use std::time::Duration;
use uppercut::api::{AnyActor, AnySender, Envelope};
use uppercut::config::{Config, SchedulerConfig};
use uppercut::core::System;
use uppercut::pool::ThreadPool;

#[derive(Debug)]
struct Message(Option<usize>, Sender<usize>);

#[derive(Default)]
struct State;

impl AnyActor for State {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(msg) = envelope.message.downcast_ref::<Message>() {
            sender.log(&format!("received: {:?}", msg));
            let x = msg.0.unwrap();
            msg.1.send(x).unwrap();
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
    let cfg = Config::new(
        SchedulerConfig {
            eager_shutdown_enabled: false,
            ..Default::default()
        },
        Default::default(),
    );
    let sys = System::new("fail-example", "localhost", &cfg);
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
