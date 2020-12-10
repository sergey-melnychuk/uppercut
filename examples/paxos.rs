use std::sync::mpsc::{channel, Sender};

extern crate uppercut;
use uppercut::api::{AnyActor, Envelope, AnySender};
use uppercut::core::System;
use uppercut::pool::ThreadPool;

enum Message {
    Request { id: u64 },
    Prepare { seq: u64 },
    Promise { seq: u64 },
    Accept { seq: u64, val: u64 },
    Accepted { seq: u64, val: u64 },
    Reject { seq: u64 },
    Selected { id: u64, val: u64 },
    Nop,
}

impl Into<Vec<u8>> for Message {
    fn into(self) -> Vec<u8> {
        Vec::new()
    }
}

impl From<&Vec<u8>> for Message {
    fn from(_: &Vec<u8>) -> Self {
        Message::Nop
    }
}

#[derive(Default)]
struct Agent {
    seq: u64,
}

impl Agent {
    fn handle(&mut self, _message: Message) -> Vec<(String, Message)> {
        Vec::new()
    }
}

enum Control {
    Loop,
    Init,
}

impl AnyActor for Agent {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(buf) = envelope.message.downcast_ref::<Vec<u8>>() {
            let message: Message = Message::from(buf);
            self.handle(message)
                .into_iter()
                .for_each(|(target, message)| {
                    let buf: Vec<u8> = message.into();
                    let envelope = Envelope::of(buf)
                        .to(&target)
                        .from(sender.me());
                    sender.send(&target, envelope);
                });
        } else if let Some(ctrl) = envelope.message.downcast_ref::<Control>() {
            match ctrl {
                Control::Init => (),
                Control::Loop => {
                    let envelope = Envelope::of(Control::Loop);
                    let me = sender.myself();
                    sender.send(&me, envelope);
                }
            }
        }
    }
}

fn main() {
    let sys = System::default();
    let pool = ThreadPool::new(6);
    let run = sys.run(&pool).unwrap();

    let peers = vec!["0", "1", "2"];
    for p in peers {
        run.spawn_default::<Agent>(p);
    }

    run.shutdown();
}
