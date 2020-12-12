use std::collections::HashSet;
use std::time::Duration;

extern crate chrono;
use chrono::Local;

extern crate bytes;
use bytes::{Bytes, Buf, BytesMut, BufMut};

extern crate uppercut;
use uppercut::api::{AnyActor, Envelope, AnySender};
use uppercut::core::System;
use uppercut::pool::ThreadPool;

#[derive(Debug, Clone, Eq, PartialEq)]
enum Message {
    Request { val: u64 },
    Prepare { seq: u64 },
    Promise { seq: u64 },
    Ignored { seq: u64, top: u64 },
    Accept { seq: u64, val: u64 },
    Accepted { seq: u64, val: u64 },
    Rejected { seq: u64 },
    Selected { seq: u64, val: u64 },
    Empty,
}

impl Into<Vec<u8>> for Message {
    fn into(self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(1 + 8 + 8);
        match self {
            Message::Request { val } => { buf.put_u8(1); buf.put_u64(val); },
            Message::Prepare { seq } => { buf.put_u8(2); buf.put_u64(seq); },
            Message::Promise { seq } => { buf.put_u8(3); buf.put_u64(seq); },
            Message::Ignored { seq, top: hi } => { buf.put_u8(4); buf.put_u64(seq); buf.put_u64(hi); },
            Message::Accept { seq, val } => { buf.put_u8(5); buf.put_u64(seq); buf.put_u64(val); },
            Message::Accepted { seq, val } => { buf.put_u8(6); buf.put_u64(seq); buf.put_u64(val); },
            Message::Rejected { seq } => { buf.put_u8(7); buf.put_u64(seq); },
            Message::Selected { seq, val } => { buf.put_u8(8); buf.put_u64(seq); buf.put_u64(val); },
            Message::Empty => buf.put_u8(0)
        };
        buf.split().to_vec()
    }
}

impl From<Vec<u8>> for Message {
    fn from(buf: Vec<u8>) -> Self {
        let mut buf = Bytes::from(buf);
        let op = buf.get_u8();
        match op {
            1 => Message::Request { val: buf.get_u64() },
            2 => Message::Prepare { seq: buf.get_u64() },
            3 => Message::Promise { seq: buf.get_u64() },
            4 => Message::Ignored { seq: buf.get_u64(), top: buf.get_u64() },
            5 => Message::Accept { seq: buf.get_u64(), val: buf.get_u64() },
            6 => Message::Accepted { seq: buf.get_u64(), val: buf.get_u64() },
            7 => Message::Rejected { seq: buf.get_u64() },
            8 => Message::Selected { seq: buf.get_u64(), val: buf.get_u64() },
            _ => Message::Empty
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_into_from_bytes() {
        let messages = vec![
            Message::Request { val: 42 },
            Message::Prepare { seq: 42 },
            Message::Promise { seq: 42 },
            Message::Ignored { seq: 42, top: 100500 },
            Message::Accept { seq: 42, val: 0xCAFEBABEDEADBEEF },
            Message::Accepted { seq: 42, val: 0xCAFEBABEDEADBEEF },
            Message::Rejected { seq: 42 },
            Message::Selected { seq: 42, val: 0xCAFEBABEDEADBEEF },
            Message::Empty,
        ];

        for message in messages {
            let buf: Vec<u8> = message.clone().into();
            let msg: Message = buf.into();
            assert_eq!(msg, message);
        }
    }
}


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
            Message::Request { val } => { // TODO propagate `val` from `Request` to `Selected`
                self.seq += 1;
                self.others()
                    .into_iter()
                    .map(|addr| (addr, Message::Prepare { seq: self.seq }))
                    .collect()
            },

            Message::Prepare { seq } => {
                if seq <= self.promised {
                    vec![(from, Message::Ignored { seq, top: self.promised })]
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
            Message::Ignored { seq: _, top: hi } => {
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
                    self.peers
                        .iter()
                        .map(|addr| (addr.clone(), Message::Selected { seq, val }))
                        .collect()
                } else {
                    vec![]
                }
            },
            Message::Rejected { seq: _ } => {
                vec![]
            },
            Message::Selected { seq: _, val } => {
                self.val = val;
                vec![]
            },
            _ => vec![]
        }
    }

    fn others(&self) -> Vec<String> {
        self.peers
            .iter()
            .filter(|addr| *addr != &self.me)
            .map(|s| s.clone())
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
            println!("{} actor={} message/local={:?}", self.time(), sender.me(), msg);
            self.handle(msg.clone(), envelope.from)
                .into_iter()
                .for_each(|(target, msg)| {
                    let buf: Vec<u8> = msg.into();
                    let envelope = Envelope::of(buf)
                        .to(&target)
                        .from(sender.me());
                    let delay = Duration::from_millis(self.delay_millis);
                    sender.delay(&target, envelope, delay);
                });
        } else if let Some(buf) = envelope.message.downcast_ref::<Vec<u8>>() {
            let msg: Message = buf.to_owned().into();
            println!("{} actor={} message/parse={:?}", self.time(), sender.me(), msg);
            self.handle(msg.clone(), envelope.from)
                .into_iter()
                .for_each(|(target, msg)| {
                    let buf: Vec<u8> = msg.into();
                    let envelope = Envelope::of(buf)
                        .to(&target)
                        .from(sender.me());
                    let delay = Duration::from_millis(self.delay_millis);
                    sender.delay(&target, envelope, delay);
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

    let values = vec![42, 101, 13];
    for val in values {
        let msg = Message::Request { val };
        let buf: Vec<u8> = msg.into();
        let envelope = Envelope::of(buf);
        run.send(peers.get(0).unwrap(), envelope);
    }

    std::thread::park();
}
