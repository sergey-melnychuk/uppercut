use std::sync::mpsc::{channel, Sender};

extern crate uppercut;
use uppercut::api::{AnyActor, AnySender, Envelope};
use uppercut::core::System;
use uppercut::config::Config;
use uppercut::pool::ThreadPool;

extern crate num_cpus;

extern crate rand;
use rand::Rng;
use std::time::Duration;

#[derive(Default)]
struct Master {
    size: usize,
    hits: usize,
    total: usize,
    result: Option<Sender<f64>>,
}

struct Pi {
    workers: usize,
    throws: usize,
    result: Sender<f64>,
}

#[derive(Default)]
struct Worker;

struct Task(usize);
struct Done(usize, usize);

impl AnyActor for Master {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(Pi { workers, throws, result }) = envelope.message.downcast_ref::<Pi>() {
            self.size = *workers;
            self.result = Some(result.clone());
            for idx in 0..self.size {
                let id = format!("worker-{}", idx);
                sender.spawn(&id, || Box::new(Worker::default()));
                let task = Envelope::of(Task(*throws)).from(sender.me());
                sender.send(&id, task);
            }
        } else if let Some(Done(hits, total)) = envelope.message.downcast_ref::<Done>() {
            self.size -= 1;
            self.hits += hits;
            self.total += total;

            if self.size == 0 {
                let pi = 4.0 * self.hits as f64 / self.total as f64;
                self.result.as_ref().iter()
                    .for_each(|tx| tx.send(pi).unwrap());
            }
        }
    }
}

impl AnyActor for Worker {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(Task(size)) = envelope.message.downcast_ref::<Task>() {
            let mut hits: usize = 0;
            let mut rng = rand::thread_rng();
            for _ in 0..*size {
                let x: f64 = rng.gen_range(0.0, 1.0);
                let y: f64 = rng.gen_range(0.0, 1.0);

                if (x*x + y*y).sqrt() <= 1.0 {
                    hits += 1;
                }
            }
            let done = Envelope::of(Done(hits, *size)).from(sender.me());
            sender.send(&envelope.from, done);
            sender.stop(&sender.myself());
        }
    }
}

fn main() {
    let cores = num_cpus::get();
    let pool = ThreadPool::new(cores + 2); // +1 event loop, +1 worker thread

    let cfg = Config::default();
    let sys = System::new("pi", &cfg);
    let run = sys.run(&pool).unwrap();

    let (tx, rx) = channel();
    run.spawn_default::<Master>("master");
    let pi = Envelope::of(Pi { workers: 10000, throws: 100000, result: tx });
    run.send("master", pi);

    let pi = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    println!("Pi estimate: {}", pi);

    run.shutdown();
}
