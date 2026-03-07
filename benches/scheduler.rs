#[macro_use]
extern crate bencher;
use bencher::Bencher;

use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};

use uppercut::api::{AnyActor, AnySender, Envelope};
use uppercut::config::{Config, SchedulerConfig};
use uppercut::core::System;
use uppercut::pool::ThreadPool;

/// One actor, 10 000 sequential self-messages.
/// Stresses the scheduler event loop; sensitive to items #1, #3, #5 in PERF.md.
fn high_throughput(b: &mut Bencher) {
    #[derive(Default)]
    struct Counter {
        count: usize,
        limit: usize,
        tx: Option<Sender<usize>>,
    }

    #[derive(Debug)]
    enum Msg {
        Init(usize, Sender<usize>),
        Hit,
    }

    impl AnyActor for Counter {
        fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
            if let Some(msg) = envelope.message.downcast_ref::<Msg>() {
                match msg {
                    Msg::Init(limit, tx) => {
                        self.limit = *limit;
                        self.tx = Some(tx.clone());
                        sender.send(sender.me(), Envelope::of(Msg::Hit));
                    }
                    Msg::Hit if self.count < self.limit => {
                        self.count += 1;
                        sender.send(sender.me(), Envelope::of(Msg::Hit));
                    }
                    Msg::Hit => {
                        self.tx.take().unwrap().send(self.count).unwrap();
                    }
                }
            }
        }
    }

    let cfg = Config {
        scheduler: SchedulerConfig {
            logging_enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let pool = ThreadPool::for_config(&cfg);

    b.iter(|| {
        let sys = System::new("bench", "localhost", &cfg);
        let run = sys.run(&pool).unwrap();

        let (tx, rx) = channel();
        run.spawn_default::<Counter>("counter");
        run.send("counter", Envelope::of(Msg::Init(10_000, tx)));

        rx.recv().unwrap();
        run.shutdown();
    });
}

/// num_cpus independent actors each running 1 000 self-messages concurrently.
/// A coordinator actor gates completion. Sensitive to item #4 in PERF.md.
fn parallel_actors(b: &mut Bencher) {
    const MESSAGES_PER_ACTOR: usize = 1_000;

    #[derive(Default)]
    struct Worker {
        count: usize,
        coordinator: String,
    }

    #[derive(Debug)]
    struct Start(String);

    #[derive(Debug)]
    struct Tick;

    #[derive(Debug)]
    struct Done;

    impl AnyActor for Worker {
        fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
            if let Some(Start(coord)) = envelope.message.downcast_ref::<Start>() {
                self.coordinator = coord.clone();
                sender.send(sender.me(), Envelope::of(Tick));
            } else if envelope.message.downcast_ref::<Tick>().is_some() {
                if self.count < MESSAGES_PER_ACTOR {
                    self.count += 1;
                    sender.send(sender.me(), Envelope::of(Tick));
                } else {
                    sender.send(&self.coordinator, Envelope::of(Done));
                }
            }
        }
    }

    struct Coordinator {
        remaining: usize,
        tx: Option<Sender<()>>,
    }

    #[derive(Debug)]
    struct CoordInit(usize, Sender<()>);

    impl AnyActor for Coordinator {
        fn receive(&mut self, envelope: Envelope, _sender: &mut dyn AnySender) {
            if let Some(CoordInit(n, tx)) = envelope.message.downcast_ref::<CoordInit>() {
                self.remaining = *n;
                self.tx = Some(tx.clone());
            } else if envelope.message.downcast_ref::<Done>().is_some() {
                self.remaining -= 1;
                if self.remaining == 0 {
                    self.tx.as_ref().unwrap().send(()).unwrap();
                }
            }
        }
    }

    let n = num_cpus::get();
    let cfg = Config {
        scheduler: SchedulerConfig {
            actor_worker_threads: n,
            logging_enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let pool = ThreadPool::for_config(&cfg);

    b.iter(|| {
        let sys = System::new("bench", "localhost", &cfg);
        let run = sys.run(&pool).unwrap();

        let (tx, rx) = channel::<()>();
        run.spawn("coordinator", move || {
            Box::new(Coordinator { remaining: 0, tx: None })
        });
        run.send("coordinator", Envelope::of(CoordInit(n, tx)));

        for i in 0..n {
            let tag = format!("worker-{}", i);
            run.spawn_default::<Worker>(&tag);
            run.send(&tag, Envelope::of(Start("coordinator".to_string())));
        }

        rx.recv().unwrap();
        run.shutdown();
    });
}

/// Delivery latency of a 1ms delayed message.
/// The system is kept alive across iterations to isolate pure delay dispatch time.
/// ns/iter ≈ actual latency; before fix #2 (PERF.md) this can reach ~256 ms/iter.
fn delay_latency(b: &mut Bencher) {
    #[derive(Default)]
    struct Sink {
        tx: Option<Sender<Instant>>,
    }

    #[derive(Debug)]
    struct SinkInit(Sender<Instant>);

    #[derive(Debug)]
    struct Ping;

    impl AnyActor for Sink {
        fn receive(&mut self, envelope: Envelope, _sender: &mut dyn AnySender) {
            if let Some(SinkInit(tx)) = envelope.message.downcast_ref::<SinkInit>() {
                self.tx = Some(tx.clone());
            } else if envelope.message.downcast_ref::<Ping>().is_some() {
                if let Some(tx) = &self.tx {
                    tx.send(Instant::now()).unwrap();
                }
            }
        }
    }

    let cfg = Config {
        scheduler: SchedulerConfig {
            logging_enabled: false,
            delay_precision: Duration::from_millis(1),
            ..Default::default()
        },
        ..Default::default()
    };
    let pool = ThreadPool::for_config(&cfg);
    let sys = System::new("bench", "localhost", &cfg);
    let run = sys.run(&pool).unwrap();

    let (tx, rx) = channel::<Instant>();
    run.spawn_default::<Sink>("sink");
    run.send("sink", Envelope::of(SinkInit(tx)));

    b.iter(|| {
        run.delay("sink", Envelope::of(Ping), Duration::from_millis(1));
        rx.recv().unwrap();
    });

    run.shutdown();
}

benchmark_group!(scheduler, high_throughput, parallel_actors, delay_latency);
benchmark_main!(scheduler);
