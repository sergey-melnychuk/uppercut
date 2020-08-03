use uppercut::pool::ThreadPool;
use uppercut::config::{Config, RemoteConfig, SchedulerConfig};
use uppercut::core::System;
use uppercut::api::{Envelope, AnyActor, AnySender};
use std::time::Duration;

#[derive(Default)]
struct PingPong;

impl AnyActor for PingPong {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(vec) = envelope.message.downcast_ref::<Vec<u8>>() {
            let msg = String::from_utf8(vec.clone()).unwrap();
            println!("PingPong: actor '{}' received '{}'\n", sender.me(), msg);

            let response = if vec == b"ping" {
                Envelope::of(b"pong".to_vec()).from(sender.me())
            } else { // vec == b"pong"
                Envelope::of(b"ping".to_vec()).from(sender.me())
            };

            sender.delay(&envelope.from, response, Duration::from_secs(1));
        }
    }
}

fn main() {
    let cores = std::cmp::max(4, num_cpus::get());
    let pool = ThreadPool::new(cores + 2 + 1);

    // sys 1

    let cfg1 = Config::new(
        SchedulerConfig::with_total_threads(cores/2),
        RemoteConfig::listening_at("0.0.0.0:10001"));
    let sys1 = System::new("one", &cfg1);
    let run1 = sys1.run(&pool).unwrap();

    run1.spawn_default::<PingPong>("ping");

    // sys 2

    let cfg2 = Config::new(
        SchedulerConfig::with_total_threads(cores/2),
        RemoteConfig::listening_at("0.0.0.0:10002"));
    let sys2 = System::new("two", &cfg2);
    let run2 = sys2.run(&pool).unwrap();

    run2.spawn_default::<PingPong>("pong");

    // send initial ping

    let env1 = Envelope::of(b"pong".to_vec())
        .to("pong@127.0.0.1:10002")
        .from("ping");
    run1.delay("client", env1, Duration::from_secs(1));

    std::thread::park();  // block current thread
}
