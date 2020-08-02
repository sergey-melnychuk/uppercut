use uppercut::pool::ThreadPool;
use uppercut::config::{Config, RemoteConfig, SchedulerConfig};
use uppercut::core::System;
use uppercut::api::{Envelope, AnyActor, AnySender};

#[derive(Default)]
struct PingPong {
    count: usize,
}

impl AnyActor for PingPong {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(s) = envelope.message.downcast_ref::<&[u8]>() {
            self.count += 1;

            println!("{}: {:?}", sender.me(), s);
            if s == b"ping\n" {
                let r = Envelope::of("pong".to_string(), sender.me());
                sender.send(&envelope.from, r);
            } else if s == b"pong\n" {
                let r = Envelope::of("ping".to_string(), sender.me());
                sender.send(&envelope.from, r);
            }
        }
    }
}

fn main() {
    let cores = num_cpus::get();
    let pool = ThreadPool::new(2 * (cores + 2) + 2);

    // sys 1

    let cfg1 = Config::new(
        SchedulerConfig::with_total_threads(cores),
        RemoteConfig::listening_at("0.0.0.0:9001"));
    let sys1 = System::new(&cfg1);
    let run1 = sys1.run(&pool).unwrap();

    run1.spawn_default::<PingPong>("pong");

    // sys 2

    let cfg2 = Config::new(
        SchedulerConfig::with_total_threads(cores),
        RemoteConfig::listening_at("0.0.0.0:9002"));
    let sys2 = System::new(&cfg2);
    let run2 = sys2.run(&pool).unwrap();

    run2.spawn_default::<PingPong>("ping");
    run2.send("pong@127.0.0.1:9001", Envelope::of(b"ping", "ping"));

    std::thread::park();
}
