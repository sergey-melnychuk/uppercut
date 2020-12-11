use std::collections::HashSet;
use std::time::Duration;

extern crate chrono;
use chrono::Local;

extern crate uppercut;
use uppercut::api::{AnyActor, Envelope, AnySender};
use uppercut::core::System;
use uppercut::pool::ThreadPool;

#[derive(Debug, Clone)]
enum Message {
    Request { val: u64 },
    Prepare { seq: u64 },
    Promise { seq: u64 },
    Ignored { seq: u64, hi: u64 },
    Accept { seq: u64, val: u64 },
    Accepted { seq: u64, val: u64 },
    Rejected { seq: u64 },
    Selected { seq: u64, val: u64 },
}

// TODO
// impl Into<Vec<u8>> for Message {
//     fn into(self) -> Vec<u8> {
//         Vec::new()
//     }
// }

// TODO
// impl From<&Vec<u8>> for Message {
//     fn from(_: &Vec<u8>) -> Self {
//         Message::Empty(0)
//     }
// }

#[derive(Default)]
struct Agent {
    me: String,
    peers: Vec<String>,
    delay_millis: u64,

    seq: u64,
    val: u64,

    promised: u64,
    accepted: u64,

    promised_by: HashSet<String>,
    accepted_by: HashSet<String>,
}

impl Agent {
    fn time(&self) -> String {
        let date = Local::now();
        date.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
    }

    fn handle(&mut self, message: Message, from: String) -> Vec<(String, Message)> {
        match message {
            Message::Request { val } => {
                self.seq += 1;
                self.val = val;
                self.others()
                    .into_iter()
                    .map(|addr| (addr, Message::Prepare { seq: self.seq }))
                    .collect()
            },

            Message::Prepare { seq } => {
                if seq <= self.promised {
                    vec![(from, Message::Ignored { seq, hi: self.promised })]
                } else {
                    self.promised = seq;
                    vec![(from, Message::Promise { seq })]
                }
            },
            Message::Promise { seq } => {
                self.promised_by.insert(from);

                let quorum = self.promised_by.len() >= self.others().len() / 2 + 1;
                if quorum {
                    self.others()
                        .into_iter()
                        .map(|addr| (addr, Message::Accept { seq, val: self.val }))
                        .collect()
                } else {
                    vec![]
                }
            },
            Message::Ignored { seq: _, hi } => {
                self.seq = hi;
                self.promised = 0;
                vec![]
            },

            Message::Accept { seq, val } => {
                if seq < self.promised {
                    vec![(from, Message::Rejected { seq })]
                } else {
                    self.seq = seq;
                    self.val = val;
                    self.promised = seq;
                    self.accepted = seq;
                    vec![(from, Message::Accepted { seq, val })]
                }
            },
            Message::Accepted { seq, val } => {
                self.accepted_by.insert(from);

                let quorum = self.accepted_by.len() >= self.others().len() / 2 + 1;
                if quorum {
                    self.others()
                        .into_iter()
                        .map(|addr| (addr, Message::Selected { seq, val }))
                        .collect()
                } else {
                    vec![]
                }
            },
            Message::Rejected { seq: _ } => {
                vec![]
            },
            Message::Selected { seq: _, val: _ } => {
                vec![]
            }
        }
    }

    fn others(&self) -> Vec<String> {
        self.peers
            .iter()
            .filter(|addr| *addr != &self.me)
            .map(|s| s.to_owned())
            .collect()
    }
}

#[derive(Debug)]
enum Control {
    Init(Vec<String>, u64),
}

impl AnyActor for Agent {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(msg) = envelope.message.downcast_ref::<Message>() {
            println!("{} actor={} message={:?}", self.time(), sender.me(), msg);
            self.handle(msg.clone(), envelope.from)
                .into_iter()
                .for_each(|(target, msg)| {
                    let envelope = Envelope::of(msg)
                        .to(&target)
                        .from(sender.me());
                    sender.delay(&target, envelope, Duration::from_millis(self.delay_millis));
                });
        } else if let Some(ctrl) = envelope.message.downcast_ref::<Control>() {
            match ctrl {
                Control::Init(peers, millis) => {
                    self.peers = peers.to_owned();
                    self.delay_millis = millis.clone();
                    self.me = sender.me().to_string();
                }
            }
        }
    }
}

fn main() {
    let sys = System::default();
    let pool = ThreadPool::new(6);
    let run = sys.run(&pool).unwrap();

    const N: usize = 3;
    let peers: Vec<String> = (0..N)
        .into_iter()
        .map(|i| format!("{}", i))
        .collect();
    let millis = 1000;

    for address in peers.iter() {
        run.spawn_default::<Agent>(address);
        let envelope = Envelope::of(Control::Init(peers.clone(), millis));
        run.send(address, envelope);
    }

    let msg = Message::Request { val: 42 };
    let envelope = Envelope::of(msg);
    run.send(peers.get(0).unwrap(), envelope);

    std::thread::park();
}
