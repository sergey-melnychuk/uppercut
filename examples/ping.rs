use std::time::Duration;

use uppercut::pool::ThreadPool;
use uppercut::config::{Config, RemoteConfig, SchedulerConfig};
use uppercut::core::System;
use uppercut::api::{Envelope, AnyActor, AnySender};

extern crate clap;
use clap::{Arg, App};

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
    let matches = App::new("pingpong")
        .arg(Arg::with_name("listen")
            .long("listen")
            .short("l")
            .required(true)
            .takes_value(true))
        .arg(Arg::with_name("peer")
            .long("peer")
            .short("p")
            .required(false)
            .takes_value(true))
        .get_matches();

    let listen = matches.value_of("listen").expect("'listen' is missing");
    let peer = matches.value_of("peer");

    let cores = std::cmp::max(4, num_cpus::get());
    let pool = ThreadPool::new(cores + 2 + 1);

    let cfg = Config::new(
        SchedulerConfig::with_total_threads(cores/2),
        RemoteConfig::listening_at(listen));
    let sys = System::new("one", &cfg);
    let run = sys.run(&pool).unwrap();

    run.spawn_default::<PingPong>("ping");

    if let Some(addr) = peer {
        let env = Envelope::of(b"ping".to_vec()).from("ping");
        run.send(addr, env);
    }

    std::thread::park();  // block current thread
}