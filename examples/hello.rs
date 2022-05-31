use std::sync::mpsc::{channel, Sender};
use std::time::Duration;

extern crate uppercut;
use uppercut::api::{AnyActor, AnySender, Envelope};
use uppercut::config::Config;
use uppercut::core::System;
use uppercut::pool::ThreadPool;

#[derive(Debug)]
struct Message(usize, Sender<usize>);

#[derive(Default)]
struct State;

impl AnyActor for State {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(msg) = envelope.message.downcast_ref::<Message>() {
            sender.log(&format!("received: {:?}", msg));
            msg.1.send(msg.0).unwrap();
        }
    }
}

fn main() {
    // Total 6 threads:
    // = 1 scheduler thread (main event loop)
    // + 4 actor-worker threads (effective parallelism level)
    // + 1 background worker thread (logging, metrics, "housekeeping")
    let tp = ThreadPool::new(6);

    let cfg = Config::default();
    let sys = System::new("basic", "localhost", &cfg);
    let run = sys.run(&tp).unwrap();

    run.spawn_default::<State>("state");

    let (tx, rx) = channel();
    run.send("state", Envelope::of(Message(42, tx)));

    let timeout = Duration::from_secs(3);
    let result = rx.recv_timeout(timeout).unwrap();
    println!("result: {}", result);
    run.shutdown();
}
