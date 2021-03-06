use std::fmt::Debug;
use std::time::Duration;
use std::sync::mpsc::{channel, Sender};

use uppercut::core::System;
use uppercut::config::{Config, SchedulerConfig, RemoteConfig, ServerConfig, ClientConfig};
use uppercut::pool::ThreadPool;
use uppercut::api::{AnyActor, Envelope, AnySender};

const TIMEOUT: Duration = Duration::from_millis(100);


#[derive(Default)]
struct Tester {
    tx: Option<Sender<Vec<u8>>>,
}

#[derive(Debug)]
struct Probe(Sender<Vec<u8>>);

#[derive(Debug)]
struct Forward(Vec<u8>, String);

impl AnyActor for Tester {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(Forward(payload, target)) = envelope.message.downcast_ref::<Forward>() {
            sender.log(&format!("Forward '{:?}' to '{}' received", payload, target));
            let envelope = Envelope::of(payload.to_vec());
            sender.send(target, envelope);
        } else if let Some(Probe(tx)) = envelope.message.downcast_ref::<Probe>() {
            sender.log("probe accepted");
            self.tx = Some(tx.clone());
        } else if let Some(vec) = envelope.message.downcast_ref::<Vec<u8>>() {
            sender.log("payload received");
            self.tx.as_ref().unwrap().send(vec.clone()).unwrap();
        }
    }
}

#[test]
fn test_remote_ping_pong() {
    let cores = 4;
    let pool = ThreadPool::new(cores + 4);

    let cfg1 = Config::new(
        SchedulerConfig::with_total_threads(cores/2),
        RemoteConfig::listening_at("127.0.0.1:10001",
                                   ServerConfig::default(),
                                   ClientConfig::default()));
    let sys1 = System::new("A", "localhost", &cfg1);
    let run1 = sys1.run(&pool).unwrap();

    let cfg2 = Config::new(
        SchedulerConfig::with_total_threads(cores/2),
        RemoteConfig::listening_at("127.0.0.1:10002",
                                   ServerConfig::default(),
                                   ClientConfig::default()));
    let sys2 = System::new("B", "localhost", &cfg2);
    let run2 = sys2.run(&pool).unwrap();

    let address = "address";
    let payload = b"hello!".to_vec();
    run1.spawn_default::<Tester>(address);
    run2.spawn_default::<Tester>(address);

    let (tx, rx) = channel();
    run2.send(address, Envelope::of(Probe(tx)));
    run1.send(address, Envelope::of(
        Forward(payload.clone(), "address@127.0.0.1:10002".to_string())));

    let result = rx.recv_timeout(TIMEOUT);
    run1.shutdown();
    run2.shutdown();
    if let Ok(received) = result {
        assert_eq!(received, payload);
    } else {
        assert!(false, "Probe did not receive forwarded payload");
    }

}