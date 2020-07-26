use std::collections::{HashSet, HashMap};
use std::time::{Instant, Duration};

extern crate uppercut;
use uppercut::api::{AnyActor, AnySender, Envelope};
use uppercut::core::System;
use uppercut::config::{Config, SchedulerConfig, RemoteConfig};
use uppercut::pool::ThreadPool;

extern crate num_cpus;

struct Round {
    size: usize,
}

impl Round {
    fn new(size: usize) -> Round {
        Round {
            size,
        }
    }
}

struct Hit(usize);

#[derive(Clone)]
struct Acc {
    name: String,
    zero: usize,
    hits: usize,
}

enum Fan {
    Trigger { size: usize },
    Out { id: usize },
    In { id: usize },
}

#[derive(Default)]
struct Root {
    size: usize,
    count: usize,
    epoch: usize,
    seen: HashSet<usize>,
}

impl AnyActor for Root {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(fan) = envelope.message.downcast_ref::<Fan>() {
            match fan {
                Fan::In { id } => {
                    self.seen.insert(*id);
                    self.count += 1;
                    if self.count == self.size {
                        self.seen.clear();
                        self.count = 0;
                        println!("root completed the fanout of size: {} (epoch: {})", self.size, self.epoch);
                        let trigger = Fan::Trigger { size: self.size };
                        let env = Envelope::of(trigger, sender.me());
                        sender.send(&sender.myself(), env);
                        self.epoch += 1;
                    }
                },
                Fan::Trigger { size } => {
                    self.size = *size;
                    for id in 0..self.size {
                        let tag = format!("{}", id);
                        let env = Envelope::of(Fan::Out { id }, sender.me());
                        sender.send(&tag, env)
                    }
                },
                _ => ()
            }
        }
    }
}

impl AnyActor for Round {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(hit) = envelope.message.downcast_ref::<Hit>() {
            let next = (hit.0 + 1) % self.size;
            let tag = format!("{}", next);
            let env = Envelope::of(Hit(hit.0 + 1), sender.me());
            sender.send(&tag, env);
        } else if let Some(acc) = envelope.message.downcast_ref::<Acc>() {
            let next = (acc.zero + acc.hits + 1) % self.size;
            let tag = format!("{}", next);
            let msg = Acc { name: acc.name.clone(), zero: acc.zero, hits: acc.hits + 1 };
            let env = Envelope::of(msg, sender.me());
            sender.send(&tag, env);
        } else if let Some(Fan::Out { id }) = envelope.message.downcast_ref::<Fan>() {
            let env = Envelope::of(Fan::In { id: *id }, sender.me());
            sender.send(&envelope.from, env);
        } else {
            println!("unexpected message: {:?}", envelope.message.type_id());
        }
    }
}

struct Periodic {
    at: Instant,
    timings: HashMap<usize, usize>,
    counter: usize,
}

impl Default for Periodic {
    fn default() -> Self {
        Periodic {
            at: Instant::now(),
            timings: HashMap::new(),
            counter: 0,
        }
    }
}

struct Tick {
    at: Instant,
}

impl AnyActor for Periodic {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(Tick { at }) = envelope.message.downcast_ref::<Tick>() {
            self.at = Instant::now();
            let d = self.at.duration_since(*at).as_millis() as usize;
            if let Some(n) = self.timings.get_mut(&d) {
                *n += 1;
            } else {
                self.timings.insert(d, 1);
            }
            self.counter += 1;
            if self.counter % 1000 == 0 {
                let total: usize = self.timings.values().sum();
                let mut ds = self.timings.keys().collect::<Vec<&usize>>();
                let mut sum: usize = 0;
                ds.sort();
                println!("timer latencies:");
                for d in ds {
                    let n = self.timings.get(d).unwrap();
                    sum += *n;
                    println!("\t{} ms\t: {}\t{}/{}", *d, *n, sum, total);
                }
                self.timings.clear();
            }
            let env = Envelope::of(Tick { at: Instant::now() }, sender.me());
            let delay = Duration::from_millis(10);
            sender.delay(&sender.myself(), env, delay);
        }
    }
}

#[derive(Default)]
struct PingPong {
    count: usize,
}

impl AnyActor for PingPong {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(s) = envelope.message.downcast_ref::<String>() {
            if self.count % 1000 == 0 {
                println!("Actor '{}' (count={}) received message '{}'", sender.myself(), self.count, s);
            }
            self.count += 1;
            if s == "ping" {
                let r = Envelope::of("pong".to_string(), sender.me());
                sender.send(&envelope.from, r);
            } else if s == "pong" {
                let r = Envelope::of("ping".to_string(), sender.me());
                sender.send(&envelope.from, r);
            }
        }
    }
}

fn main() {
    let cores = num_cpus::get();
    let pool = ThreadPool::new(cores + 2); // +1 event loop, +1 worker thread

    let cfg = Config::new(
        SchedulerConfig::with_total_threads(cores),
        RemoteConfig::default());
    let sys = System::new(cfg);
    let run = sys.run(&pool).unwrap();

    const SIZE: usize = 100_000;
    for id in 0..SIZE {
        let tag = format!("{}", id);
        run.spawn(&tag, || Box::new(Round::new(SIZE)));
    }

    run.send("0", Envelope::of(Hit(0), ""));

    for id in 0..1000 {
        let tag = format!("{}", id);
        let acc = Acc { name: tag.clone(), zero: id, hits: 0 };
        let env = Envelope::of(acc, &tag);
        run.send(&tag, env);
    }

    run.spawn_default::<Root>("root");
    let env = Envelope::of(Fan::Trigger { size: SIZE }, "root");
    run.send("root", env);

    run.spawn_default::<Periodic>("timer");
    let tick = Envelope::of(Tick { at: Instant::now() }, "timer");
    run.delay("timer", tick, Duration::from_secs(10));

    run.spawn_default::<PingPong>("ping");
    run.spawn_default::<PingPong>("pong");

    let ping = Envelope::of("ping".to_string(), "pong");
    run.send("ping", ping);

    std::thread::park(); // block current thread (https://doc.rust-lang.org/std/thread/fn.park.html)
}
