use std::time::Duration;

use uppercut::api::{AnyActor, AnySender, Envelope};
use uppercut::config::{ClientConfig, Config, RemoteConfig, SchedulerConfig, ServerConfig};
use uppercut::core::System;
use uppercut::pool::ThreadPool;

extern crate clap;
use clap::{Arg, Command};

#[derive(Default)]
struct PingPong;

impl AnyActor for PingPong {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(vec) = envelope.message.downcast_ref::<Vec<u8>>() {
            let msg = String::from_utf8(vec.clone()).unwrap();
            println!("PingPong: actor '{}' received '{}'\n", sender.me(), msg);

            let response = if vec == b"ping" {
                Envelope::of(b"pong".to_vec()).from(sender.me())
            } else {
                // vec == b"pong"
                Envelope::of(b"ping".to_vec()).from(sender.me())
            };

            sender.delay(&envelope.from, response, Duration::from_secs(1));
        }
    }
}

// cargo run --example ping -- --listen 0.0.0.0:10001
// cargo run --example ping -- --listen 0.0.0.0:10002 --peer ping@127.0.0.1:10001
fn main() {
    let matches = Command::new("pingpong")
        .arg(Arg::new("listen").long("listen").short('l').required(true))
        .arg(Arg::new("peer").long("peer").short('p').required(false))
        .get_matches();

    let listen = matches
        .get_one::<String>("listen")
        .expect("'listen' is missing");
    let peer = matches.get_one::<String>("peer");

    let cores = std::cmp::max(4, num_cpus::get());
    let pool = ThreadPool::new(cores + 2 + 1);

    let cfg = Config::new(
        SchedulerConfig::with_total_threads(cores / 2),
        RemoteConfig::listening_at(listen, ServerConfig::default(), ClientConfig::default()),
    );
    let sys = System::new("one", "localhost", &cfg);
    let run = sys.run(&pool).unwrap();

    run.spawn_default::<PingPong>("ping");

    if let Some(addr) = peer {
        let env = Envelope::of(b"ping".to_vec()).from("ping");
        run.send(addr, env);
    }

    std::thread::park(); // block current thread
}
