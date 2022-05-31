#[macro_use]
extern crate bencher;
use bencher::Bencher;

use std::sync::mpsc::{channel, Sender};

use uppercut::api::{AnyActor, AnySender, Envelope};
use uppercut::config::Config;
use uppercut::core::System;
use uppercut::pool::ThreadPool;

fn counter(b: &mut Bencher) {
    #[derive(Default)]
    struct Test {
        count: usize,
        limit: usize,
        tx: Option<Sender<usize>>,
    }

    #[derive(Debug)]
    enum Protocol {
        Init(usize, Sender<usize>),
        Hit,
    }

    impl AnyActor for Test {
        fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
            if let Some(p) = envelope.message.downcast_ref::<Protocol>() {
                let me = sender.myself();
                match p {
                    Protocol::Init(limit, tx) => {
                        self.limit = *limit;
                        self.tx = Some(tx.to_owned());
                        sender.send(&me, Envelope::of(Protocol::Hit).from(&me));
                    }
                    Protocol::Hit if self.count < self.limit => {
                        self.count += 1;
                        sender.send(&me, Envelope::of(Protocol::Hit).from(&me));
                    }
                    Protocol::Hit => {
                        self.tx.take().unwrap().send(self.count).unwrap();
                    }
                }
            }
        }
    }

    let cfg = Config::default();
    let pool: ThreadPool = ThreadPool::for_config(&cfg);
    b.iter(|| {
        let sys = System::new("bench", "localhost", &cfg);
        let run = sys.run(&pool).unwrap();

        let (tx, rx) = channel();
        run.spawn_default::<Test>("test");
        run.send("test", Envelope::of(Protocol::Init(1000, tx)));

        rx.recv().unwrap();

        run.shutdown();
    });
}

fn chain(b: &mut Bencher) {
    const LENGTH: usize = 1000;

    #[derive(Debug)]
    struct Hit(Sender<usize>, usize);

    #[derive(Default)]
    struct Chain;

    impl AnyActor for Chain {
        fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
            if let Some(Hit(tx, hits)) = envelope.message.downcast_ref::<Hit>() {
                if *hits < LENGTH {
                    let tag = format!("{}", hits + 1);
                    sender.spawn(&tag, || Box::new(Chain));
                    let env = Envelope::of(Hit(tx.to_owned(), hits + 1));
                    sender.send(&tag, env);
                } else {
                    tx.send(*hits).unwrap();
                }
            }
        }
    }

    let cfg = Config::default();
    let pool: ThreadPool = ThreadPool::for_config(&cfg);
    b.iter(|| {
        let sys = System::new("bench", "localhost", &cfg);
        let run = sys.run(&pool).unwrap();

        let (tx, rx) = channel();
        run.spawn_default::<Chain>("0");
        run.send("0", Envelope::of(Hit(tx, 0)));

        rx.recv().unwrap();

        run.shutdown();
    });
}

benchmark_group!(workload, counter, chain);
benchmark_main!(workload);
