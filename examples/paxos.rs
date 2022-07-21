use std::collections::HashSet;
use std::time::Duration;

extern crate log;
use log::{debug, info};
use env_logger::fmt::TimestampPrecision;

extern crate bytes;
use bytes::{Buf, BufMut, Bytes, BytesMut};

use crossbeam_channel::{bounded, Sender};

extern crate uppercut;
use uppercut::api::{AnyActor, AnySender, Envelope};
use uppercut::config::Config;
use uppercut::core::{Run, System};
use uppercut::pool::ThreadPool;

const SEND_DELAY_MILLIS: u64 = 20;

#[derive(Debug, Clone, Eq, PartialEq)]
enum Message {
    Request { val: u64 },
    Prepare { seq: u64 },
    Promise { seq: u64 },
    Ignored { seq: u64 },
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
            Message::Request { val } => {
                buf.put_u8(1);
                buf.put_u64(val);
            }
            Message::Prepare { seq } => {
                buf.put_u8(2);
                buf.put_u64(seq);
            }
            Message::Promise { seq } => {
                buf.put_u8(3);
                buf.put_u64(seq);
            }
            Message::Ignored { seq } => {
                buf.put_u8(4);
                buf.put_u64(seq);
            }
            Message::Accept { seq, val } => {
                buf.put_u8(5);
                buf.put_u64(seq);
                buf.put_u64(val);
            }
            Message::Accepted { seq, val } => {
                buf.put_u8(6);
                buf.put_u64(seq);
                buf.put_u64(val);
            }
            Message::Rejected { seq } => {
                buf.put_u8(7);
                buf.put_u64(seq);
            }
            Message::Selected { seq, val } => {
                buf.put_u8(8);
                buf.put_u64(seq);
                buf.put_u64(val);
            }
            Message::Empty => buf.put_u8(0),
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
            4 => Message::Ignored { seq: buf.get_u64() },
            5 => Message::Accept {
                seq: buf.get_u64(),
                val: buf.get_u64(),
            },
            6 => Message::Accepted {
                seq: buf.get_u64(),
                val: buf.get_u64(),
            },
            7 => Message::Rejected { seq: buf.get_u64() },
            8 => Message::Selected {
                seq: buf.get_u64(),
                val: buf.get_u64(),
            },
            _ => Message::Empty,
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
            Message::Ignored { seq: 42 },
            Message::Accept {
                seq: 42,
                val: 0xCAFEBABEDEADBEEF,
            },
            Message::Accepted {
                seq: 42,
                val: 0xCAFEBABEDEADBEEF,
            },
            Message::Rejected { seq: 42 },
            Message::Selected {
                seq: 42,
                val: 0xCAFEBABEDEADBEEF,
            },
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
    clients: HashSet<String>,

    val: u64,
    seq: u64,
    seq_promised: u64,
    seq_accepted: u64,
    promised: HashSet<String>,
    accepted: HashSet<String>,
    storage: Vec<(u64, u64)>,
}

impl Agent {
    fn handle(&mut self, message: Message, from: String) -> Vec<(String, Message)> {
        match message {
            Message::Request { val } => {
                self.val = val;
                self.clients.insert(from);
                self.peers
                    .iter()
                    .map(|tag| (tag.clone(), Message::Prepare { seq: self.seq + 1 }))
                    .collect()
            }
            Message::Prepare { seq } => {
                if seq > self.seq {
                    self.seq = seq;
                    vec![(from, Message::Promise { seq: self.seq })]
                } else {
                    vec![(from, Message::Ignored { seq })]
                }
            }
            Message::Promise { seq } if seq > self.seq_promised => {
                self.promised.insert(from);
                if self.quorum(self.promised.len()) {
                    self.seq_promised = seq;
                    self.promised.clear();
                    let msg = Message::Accept { seq, val: self.val };
                    self.peers
                        .iter()
                        .map(|tag| (tag.clone(), msg.clone()))
                        .collect()
                } else {
                    vec![]
                }
            }
            Message::Ignored { seq: _ } => {
                vec![]
            }
            Message::Accept { seq, val } => {
                if seq >= self.seq {
                    vec![(from, Message::Accepted { seq, val })]
                } else {
                    vec![(from, Message::Rejected { seq })]
                }
            }
            Message::Accepted { seq, val } if seq > self.seq_accepted => {
                self.accepted.insert(from);
                if self.quorum(self.accepted.len()) {
                    self.seq_accepted = seq;
                    self.accepted.clear();
                    let msg = Message::Selected { seq, val };
                    self.peers
                        .iter()
                        .map(|tag| (tag.clone(), msg.clone()))
                        .collect()
                } else {
                    vec![]
                }
            }
            Message::Rejected { seq: _ } => {
                vec![]
            }
            Message::Selected { seq, val } => {
                self.storage.push((seq, val));
                self.seq = seq;
                self.promised.clear();
                self.clients
                    .iter()
                    .map(|tag| (tag.clone(), Message::Selected { seq, val }))
                    .collect()
            }
            _ => vec![],
        }
    }

    fn quorum(&self, n: usize) -> bool {
        n > self.peers.len() / 2
    }
}

#[derive(Debug)]
enum Control {
    Init(Vec<String>),
}

impl AnyActor for Agent {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(buf) = envelope.message.downcast_ref::<Vec<u8>>() {
            let msg: Message = buf.to_owned().into();
            info!(
                "actor={} from={} message/parse={:?}",
                sender.me(),
                envelope.from,
                msg
            );
            self.handle(msg.clone(), envelope.from)
                .into_iter()
                .for_each(|(target, msg)| {
                    info!("\t{} sending to {}: {:?}", sender.me(), target, msg);
                    let buf: Vec<u8> = msg.into();
                    let envelope = Envelope::of(buf).to(&target).from(sender.me());
                    let delay = Duration::from_millis(SEND_DELAY_MILLIS);
                    sender.delay(&target, envelope, delay);
                });
        } else if let Some(ctrl) = envelope.message.downcast_ref::<Control>() {
            match ctrl {
                Control::Init(peers) => {
                    self.peers = peers.to_owned();
                    self.me = sender.me().to_string();
                    debug!(
                        "tag={} init: peers={:?} me={}",
                        sender.me(),
                        self.peers,
                        self.me
                    );
                }
            }
        }
    }
}

#[derive(Debug, Default)]
struct Client {
    n: u32,
    id: u64,
    val: u64,
    done: bool,
    nodes: Vec<String>,
    log: Vec<u64>,
    sender: Option<Sender<(String, Vec<u64>)>>,
}

#[derive(Debug)]
struct Setup(u32, u64, u64, Vec<String>, Sender<(String, Vec<u64>)>);

impl AnyActor for Client {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(buf) = envelope.message.downcast_ref::<Vec<u8>>() {
            let message: Message = buf.to_owned().into();
            info!("actor={} message={:?}", sender.me(), message);
            match message {
                Message::Selected { seq: _, val } => {
                    self.log.push(val);
                    self.done |= val == self.val;
                    if !self.done {
                        let idx: usize = self.id as usize % self.nodes.len();
                        let target = self.nodes.get(idx).unwrap();
                        let msg = Message::Request { val: self.val };
                        info!("actor={} retry/message={:?}", sender.me(), msg);
                        let buf: Vec<u8> = msg.into();
                        let envelope = Envelope::of(buf).from(sender.me());
                        sender.send(target, envelope);
                    }

                    if self.log.len() == self.n as usize {
                        self.sender
                            .as_ref()
                            .unwrap()
                            .send((sender.me().to_string(), self.log.clone()))
                            .unwrap()
                    }
                }
                _ => (),
            }
        } else if let Some(setup) = envelope.message.downcast_ref::<Setup>() {
            let Setup(n, id, val, nodes, tx) = setup;
            self.n = *n;
            self.id = *id;
            self.val = *val;
            self.done = false;
            self.nodes = nodes.to_owned();
            self.sender = Some(tx.to_owned());
            info!(
                "actor={} val={} seq={:?}",
                sender.me(),
                self.val,
                self.log
            );

            let idx: usize = self.id as usize % self.nodes.len();
            let target = self.nodes.get(idx).unwrap();
            let msg = Message::Request { val: self.val };
            let buf: Vec<u8> = msg.into();
            let envelope = Envelope::of(buf).from(sender.me());
            sender.send(target, envelope);
        }
    }
}

// RUST_LOG=info cargo run --release --example paxos
fn main() {
    env_logger::builder()
        .format_timestamp(Some(TimestampPrecision::Millis))
        .init();
    let pool = ThreadPool::new(6);

    let mut config = Config::default();
    config.scheduler.actor_worker_threads = 1;
    config.scheduler.extra_worker_threads = 0;
    config.remote.enabled = true;

    let runs: Vec<Run> = {
        config.remote.listening = "127.0.0.1:9001".to_string();
        let sys1 = System::new("paxos-1", "localhost", &config);
        let run1 = sys1.run(&pool).unwrap();

        config.remote.listening = "127.0.0.1:9002".to_string();
        let sys2 = System::new("paxos-2", "localhost", &config);
        let run2 = sys2.run(&pool).unwrap();

        config.remote.listening = "127.0.0.1:9003".to_string();
        let sys3 = System::new("paxos-3", "localhost", &config);
        let run3 = sys3.run(&pool).unwrap();

        vec![run1, run2, run3]
    };

    const N: usize = 3;
    let peers: Vec<String> = (0..N)
        .zip(9001..(9001 + N))
        .into_iter()
        .map(|(i, port)| format!("node-{}@127.0.0.1:{}", i, port))
        .collect();

    for (address, run) in peers.iter().zip(runs.iter()) {
        let tag = address.split('@').next().unwrap();
        run.spawn_default::<Agent>(tag);
        let envelope = Envelope::of(Control::Init(peers.clone()));
        run.send(tag, envelope);
    }

    let clients = vec![("client-A", 30), ("client-B", 73), ("client-C", 42)];
    let (tx, rx) = bounded(clients.len());

    for ((id, (tag, val)), run) in clients.clone().into_iter().enumerate().zip(runs.iter()) {
        run.spawn_default::<Client>(tag);
        let setup = Setup(
            clients.len() as u32,
            id as u64,
            val,
            peers.clone(),
            tx.clone(),
        );
        let envelope = Envelope::of(setup);
        run.send(tag, envelope);
        println!("{}: {}", tag, val);
    }

    let mut seen: HashSet<Vec<u64>> = HashSet::with_capacity(clients.len() + 1);
    for _ in 0..clients.len() {
        if let Ok(received) = rx.recv_timeout(Duration::from_secs(20)) {
            println!("{:?}", received);
            seen.insert(received.1);
        } else {
            break;
        }
    }

    runs.into_iter().for_each(|run| run.shutdown());

    let ok = {
        let numbers: Vec<u64> = clients.iter().map(|(_, n)| *n).collect();
        seen.iter()
            .all(|vec| numbers.iter().all(|x| vec.contains(x)))
    };
    if seen.len() == 1 && ok {
        println!("OK");
        std::process::exit(0);
    } else {
        println!("FAILED");
        std::process::exit(1);
    }
}
