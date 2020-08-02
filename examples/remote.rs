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
        println!("ping-pong: received");
        if let Some(vec) = envelope.message.downcast_ref::<Vec<u8>>() {
            self.count += 1;

            println!("PingPong me={} e.to={} e.from={} e.vec={:?}/{}",
                     sender.me(), envelope.to, envelope.from, vec, String::from_utf8(vec.clone()).unwrap());
            if vec == b"ping" {
                // let r = Envelope::of(b"pong".to_vec()).from(sender.me());
                // sender.send(&envelope.from, r);

                println!("received ping");
                let r = Envelope::of(b"pong".to_vec()).from("ping").to("pong@127.0.0.1:10002");
                println!("response: r.to={} r.from={}", r.to, r.from);
                sender.send("client", r);
                // TODO FIXME envelopes sent from Actor (not Run) are never received by the Client!

            } else if vec == b"pong" {
                // let r = Envelope::of(b"ping".to_vec()).from(sender.me());
                // sender.send(&envelope.from, r);

                println!("received pong");
                let r = Envelope::of(b"ping".to_vec()).from("pong").to("ping@127.0.0.1:10001");
                println!("response: r.to={} r.from={}", r.to, r.from);
                sender.send("client", r);
                // TODO FIXME (same problem)

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
        RemoteConfig::listening_at("0.0.0.0:10001"));
    let sys1 = System::new(&cfg1);
    let run1 = sys1.run(&pool).unwrap();

    run1.spawn_default::<PingPong>("ping");

    // run1.spawn_default::<PingPong>("ping");
    // run1.send("pong", Envelope::of(b"ping".to_vec()).from("ping"));

    // sys 2

    let cfg2 = Config::new(
        SchedulerConfig::with_total_threads(cores),
        RemoteConfig::listening_at("0.0.0.0:10002"));
    let sys2 = System::new(&cfg2);
    let run2 = sys2.run(&pool).unwrap();

    run2.spawn_default::<PingPong>("pong");
    //run2.send("pong@127.0.0.1:10001", Envelope::of(b"ping".to_vec()).from("ping"));

    //

    let e1 = Envelope::of(b"pong".to_vec()).to("pong@127.0.0.1:10002").from("ping");
    run1.send("client", e1);

    // let e2 = Envelope::of(b"ping".to_vec()).to("ping@127.0.0.1:10001").from("pong");
    // run2.send("client", e2);

    std::thread::park();
}
