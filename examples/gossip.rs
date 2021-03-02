extern crate log;
use log::{debug, info, warn};

extern crate bytes;
use bytes::{Bytes, Buf, BytesMut, BufMut};

use crossbeam_channel::{bounded, Sender};

extern crate uppercut;
use uppercut::api::{AnyActor, Envelope, AnySender};
use uppercut::config::{Config};
use uppercut::core::{Run, System};
use uppercut::pool::ThreadPool;
use std::time::{Instant, Duration};


#[derive(Debug, Default, Clone)]
struct Peer {
    tag: String,
    beat: u64,
    seen: u64,
}

#[derive(Debug)]
struct Agent {
    tag: String,
    beat: u64,
    peers: Vec<Peer>,
    horizon: u64,
    period: u64,
    tick: u64,
    zero: Instant,
    countdown: String,
}

impl Default for Agent {
    fn default() -> Self {
        Self {
            tag: Default::default(),
            beat: Default::default(),
            peers: Default::default(),
            horizon: Default::default(),
            period: Default::default(),
            tick: Default::default(),
            zero: Instant::now(),
            countdown: Default::default(),
        }
    }
}

#[derive(Debug)]
enum Event {
    New(String),
    Out(String),
}

impl Agent {
    fn gossip(&self, time: u64) -> Vec<(String, u64)> {
        let peers: Vec<(String, u64)> = self.peers
            .iter()
            .filter(|peer| peer.seen + self.horizon >= time)
            .map(|peer| (peer.tag.clone(), peer.beat))
            .collect();
        if peers.len() < 2 {
            return vec![(self.tag.clone(), self.beat)];
        } else {
            vec![
                peers[self.beat as usize % peers.len()].clone(),
                (self.tag.clone(), self.beat),
            ]
        }
    }

    fn detect(&self, time: u64) -> Vec<Event> {
        let mut events = Vec::new();
        for peer in self.peers.iter() {
            if peer.seen + self.horizon < time {
                events.push(Event::Out(peer.tag.clone()));
            }
        }
        events
    }

    fn accept(&mut self, time: u64, gossip: Vec<(String, u64)>) -> Vec<Event> {
        let mut events = Vec::new();
        for (tag, beat) in gossip {
            if tag == self.tag {
                continue;
            }
            let known = self.peers.iter_mut().find(|p| p.tag == tag);
            if let Some(peer) = known {
                if beat > peer.beat {
                    peer.beat = beat;
                    peer.seen = time;
                }
            } else {
                // new (previously unseen) peer is introduced through the gossip
                events.push(Event::New(tag.clone()));
                self.peers.push(Peer { tag,  beat, seen: time });
            }
        }

        events.append(&mut self.detect(time));
        events
    }
}

#[derive(Debug, Clone)]
enum Message {
    Ping(String, u64),
    Pong(String, u64),
    Gossip(Vec<(String, u64)>),
    Stop,

    Tick,
    Init(u64, u64, u64, Vec<String>, String),
}

impl Into<Vec<u8>> for Message {
    fn into(self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        match self {
            Message::Ping(target, beat) => {
                buf.put_u8(1);
                buf.put_u8(target.len() as u8);
                buf.put_slice(target.as_bytes());
                buf.put_u64(beat);
            },
            Message::Pong(target, beat) => {
                buf.put_u8(2);
                buf.put_u8(target.len() as u8);
                buf.put_slice(target.as_bytes());
                buf.put_u64(beat);
            },
            Message::Gossip(pairs) => {
                buf.put_u8(3);
                buf.put_u8(pairs.len() as u8);
                pairs.into_iter()
                    .for_each(|(tag, beat)| {
                        buf.put_u8(tag.len() as u8);
                        buf.put_slice(tag.as_bytes());
                        buf.put_u64(beat);
                    });
            }
            Message::Stop => { buf.put_u8(4); },
            _ => { buf.put_u8(0); }
        }
        buf.split().to_vec()
    }
}

impl From<Vec<u8>> for Message {
    fn from(vec: Vec<u8>) -> Self {
        let mut buf = Bytes::from(vec);
        let op = buf.get_u8();
        match op {
            1 => {
                let n = buf.get_u8();
                let v = buf.copy_to_bytes(n as usize).to_vec();
                let b = buf.get_u64();
                Message::Ping(String::from_utf8(v).unwrap(), b)
            },
            2 => {
                let n = buf.get_u8();
                let v = buf.copy_to_bytes(n as usize).to_vec();
                let b = buf.get_u64();
                Message::Pong(String::from_utf8(v).unwrap(), b)
            },
            3 => {
                let k = buf.get_u8();
                let peers: Vec<(String, u64)> = (0..k)
                    .into_iter()
                    .map(|_| {
                        let n = buf.get_u8();
                        let v = buf.copy_to_bytes(n as usize).to_vec();
                        let b = buf.get_u64();
                        (String::from_utf8(v).unwrap(), b)
                    })
                    .collect();
                Message::Gossip(peers)
            },
            4 => Message::Stop,
            _ => Message::Tick
        }
    }
}

impl AnyActor for Agent {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        let time = self.zero.elapsed().as_millis() as u64;
        if let Some(buf) = envelope.message.downcast_ref::<Vec<u8>>() {
            let message: Message = buf.clone().into();
            info!("tag={} time={} msg={:?} from={}\n\tpeers={:?}", sender.me(), time, message, envelope.from, self.peers);
            match message {
                Message::Gossip(peers) => {
                    let events = self.accept(time, peers);
                    for e in events {
                        info!("\ttag={} event={:?}", sender.me(), e);
                    }
                },
                Message::Ping(me, beat) => {
                    self.tag = me;
                    let is_known = self.peers.iter().any(|p| p.tag == sender.me());
                    let is_self = sender.me() == envelope.from;
                    if !is_self && !is_known {
                        let pong: Vec<u8> = Message::Pong(envelope.from.clone(), self.beat).into();
                        sender.send(&envelope.from, Envelope::of(pong).from(sender.me()));
                        let events = self.accept(time, vec![(envelope.from, beat)]);
                        for e in events {
                            info!("\ttag={} event={:?}", sender.me(), e);
                        }
                    }
                },
                Message::Pong(me, beat) => {
                    self.tag = me;
                    let is_known = self.peers.iter().any(|p| p.tag == sender.me());
                    let is_self = sender.me() == envelope.from;
                    if !is_self && !is_known {
                        let events = self.accept(time, vec![(envelope.from, beat)]);
                        for e in events {
                            info!("\ttag={} event={:?}", sender.me(), e);
                        }
                    }
                },
                Message::Stop => {
                    warn!("tag={} event=stop", sender.me());
                    sender.stop(&sender.myself());
                    let down: Vec<u8> = Down(sender.myself()).into();
                    sender.send(&self.countdown, Envelope::of(down));
                },
                _ => ()
            }
        } else if let Some(Message::Tick) = envelope.message.downcast_ref::<Message>() {
            self.beat += 1;
            debug!("tag={} beat={} msg=Tick", sender.me(), self.beat);
            if self.beat % self.period == 0 {
                let events = self.detect(time);
                for e in events {
                    info!("\ttag={} event={:?}", sender.me(), e);
                }
                let gossip = self.gossip(time);
                info!("tag={} gossip: {:?}", sender.me(), gossip);
                let gossip: Vec<u8> = Message::Gossip(gossip).into();
                self.peers
                    .iter()
                    .for_each(|p| {
                        let envelope = Envelope::of(gossip.clone()).from(sender.me());
                        sender.send(&p.tag, envelope);
                    });
            }
            let delay = Duration::from_millis(self.tick);
            let myself = sender.myself();
            sender.delay(&myself, Envelope::of(Message::Tick), delay);
        } else if let Some(Message::Init(horizon, tick, period, pings, countdown)) = envelope.message.downcast_ref::<Message>() {
            debug!("tag={} msg=Init(horizon={} period={} tick={})", sender.me(), horizon, period, tick);
            self.horizon = *horizon;
            self.tick = *tick;
            self.period = *period;
            self.countdown = countdown.to_owned();
            sender.send(&sender.myself(), Envelope::of(Message::Tick));

            pings.into_iter()
                .for_each(|tag| {
                    let ping: Vec<u8> = Message::Ping(tag.clone(), self.beat).into();
                    sender.send(tag, Envelope::of(ping).from(sender.me()));
                })
        }
    }
}

#[derive(Default)]
struct Countdown {
    n: usize,
    tx: Option<Sender<()>>,
}

#[derive(Debug)]
struct Setup(usize, Sender<()>);

#[derive(Debug)]
struct Down(String);

impl Into<Vec<u8>> for Down {
    fn into(self) -> Vec<u8> {
        self.0.into_bytes()
    }
}

impl From<Vec<u8>> for Down {
    fn from(vec: Vec<u8>) -> Self {
        Down(String::from_utf8(vec).unwrap())
    }
}

impl AnyActor for Countdown {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(Setup(n, tx)) = envelope.message.downcast_ref::<Setup>() {
            self.n = *n;
            self.tx = Some(tx.to_owned());
        } else if let Some(vec) = envelope.message.downcast_ref::<Vec<u8>>() {
            let Down(tag) = vec.to_owned().into();
            warn!("countdown: down='{}'", tag);
            self.n -= 1;
            if self.n == 0 {
                self.tx.as_ref().unwrap().send(()).unwrap();
                sender.stop(&sender.myself());
            }
        }
    }
}

// RUST_LOG=info cargo run --release --example gossip --features remote
fn main() {
    env_logger::init();
    let pool = ThreadPool::new(6);

    let mut config = Config::default();
    config.scheduler.actor_worker_threads = 1;
    config.scheduler.extra_worker_threads = 0;
    config.remote.enabled = true;

    let runs: Vec<Run> = {
        config.remote.listening = "127.0.0.1:9101".to_string();
        let sys1 = System::new("gossip-1", "localhost", &config);
        let run1 = sys1.run(&pool).unwrap();

        config.remote.listening = "127.0.0.1:9102".to_string();
        let sys2 = System::new("gossip-2", "localhost", &config);
        let run2 = sys2.run(&pool).unwrap();

        config.remote.listening = "127.0.0.1:9103".to_string();
        let sys3 = System::new("gossip-3", "localhost", &config);
        let run3 = sys3.run(&pool).unwrap();

        vec![run1, run2, run3]
    };

    let r = runs.get(0).unwrap();
    r.spawn_default::<Countdown>("countdown");
    let (tx, rx) = bounded(1);
    r.send("countdown", Envelope::of(Setup(runs.len(), tx)));

    let (horizon, tick, period) = (1000, 100, 3);
    for (id, run) in runs.iter().enumerate() {
        let tag = format!("peer-{}", id);
        run.spawn_default::<Agent>(&tag);
        let pings = if id == 0 {
            vec![]
        } else {
            vec!["peer-0@127.0.0.1:9101".to_string()]
        };
        let init = Message::Init(horizon, period, tick, pings, "countdown@127.0.0.1:9101".to_string());
        run.send(&tag, Envelope::of(init));

        let millis = horizon * 5 * (1 + id as u64);
        let delay = Duration::from_millis(millis);
        let stop: Vec<u8> = Message::Stop.into();
        run.delay(&tag, Envelope::of(stop), delay);
    }

    let _ = rx.recv().unwrap(); // wait until all 3 peers are down
    runs.into_iter()
        .for_each(|run| run.shutdown());
}
