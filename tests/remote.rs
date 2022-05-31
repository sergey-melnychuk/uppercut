#[cfg(feature = "remote")]
mod ping_test {
    use std::fmt::Debug;
    use std::sync::mpsc::Sender;

    use uppercut::api::{AnyActor, AnySender, Envelope};

    #[derive(Default)]
    pub struct Tester {
        pub tx: Option<Sender<Vec<u8>>>,
    }

    #[derive(Debug)]
    pub struct Probe(pub Sender<Vec<u8>>);

    #[derive(Debug)]
    pub struct Forward(pub Vec<u8>, pub String);

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
}

#[cfg(feature = "remote")]
#[test]
fn test_remote_ping_pong() {
    use crate::ping_test::{Forward, Probe, Tester};
    use std::sync::mpsc::channel;
    use std::time::Duration;
    use uppercut::api::Envelope;
    use uppercut::config::{ClientConfig, Config, RemoteConfig, SchedulerConfig, ServerConfig};
    use uppercut::core::System;
    use uppercut::pool::ThreadPool;

    const TIMEOUT: Duration = Duration::from_millis(300);

    let cores = 4;
    let pool = ThreadPool::new(cores + 4);

    let cfg1 = Config::new(
        SchedulerConfig::with_total_threads(cores / 2),
        RemoteConfig::listening_at(
            "127.0.0.1:10001",
            ServerConfig::default(),
            ClientConfig::default(),
        ),
    );
    let sys1 = System::new("A", "localhost", &cfg1);
    let run1 = sys1.run(&pool).unwrap();

    let cfg2 = Config::new(
        SchedulerConfig::with_total_threads(cores / 2),
        RemoteConfig::listening_at(
            "127.0.0.1:10002",
            ServerConfig::default(),
            ClientConfig::default(),
        ),
    );
    let sys2 = System::new("B", "localhost", &cfg2);
    let run2 = sys2.run(&pool).unwrap();

    let address = "address";
    let payload = b"hello!".to_vec();
    run1.spawn_default::<Tester>(address);
    run2.spawn_default::<Tester>(address);

    let (tx, rx) = channel();
    run2.send(address, Envelope::of(Probe(tx)));
    run1.send(
        address,
        Envelope::of(Forward(
            payload.clone(),
            "address@127.0.0.1:10002".to_string(),
        )),
    );

    let result = rx.recv_timeout(TIMEOUT);
    run1.shutdown();
    run2.shutdown();
    if let Ok(received) = result {
        assert_eq!(received, payload);
    } else {
        assert!(false, "Probe did not receive forwarded payload");
    }
}

#[cfg(feature = "remote")]
mod reply_test {
    use std::fmt::Debug;
    use std::sync::mpsc::Sender;
    use uppercut::api::{AnyActor, AnySender, Envelope};

    #[derive(Default)]
    pub struct Spy {
        pub tx: Option<Sender<String>>,
    }

    #[derive(Debug)]
    pub struct Probe {
        pub tx: Sender<String>,
        pub message: String,
        pub target: String,
    }

    impl AnyActor for Spy {
        fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
            if let Some(probe) = envelope.message.downcast_ref::<Probe>() {
                sender.log("probe received");
                self.tx = Some(probe.tx.clone());
                let env = Envelope::of(probe.message.as_bytes().to_vec())
                    .to(&probe.target)
                    .from(sender.me());
                sender.log(&format!(
                    "sent message '{}' to '{}'",
                    probe.message, probe.target
                ));
                sender.send(&probe.target, env);
            } else if let Some(buf) = envelope.message.downcast_ref::<Vec<u8>>() {
                sender.log("response received");
                let message = String::from_utf8(buf.clone()).unwrap_or("<undefined>".to_string());
                self.tx.as_ref().unwrap().send(message.clone()).unwrap();
            }
        }
    }

    #[derive(Default)]
    pub struct Echo;

    impl AnyActor for Echo {
        fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
            if let Some(buf) = envelope.message.downcast_ref::<Vec<u8>>() {
                sender.log(&format!("received envelope from {}", envelope.from));
                let reply = Envelope::of(buf.to_vec())
                    .to(&envelope.from)
                    .from(&envelope.to);
                sender.log(&format!("sending echo to '{}'", reply.to));
                sender.send(&envelope.from, reply);
            }
        }
    }
}

#[cfg(feature = "remote")]
#[test]
fn test_remote_reply() {
    use crate::reply_test::*;
    use std::sync::mpsc::channel;
    use std::time::Duration;
    use uppercut::api::Envelope;
    use uppercut::config::{ClientConfig, Config, RemoteConfig, SchedulerConfig, ServerConfig};
    use uppercut::core::System;
    use uppercut::pool::ThreadPool;

    const TIMEOUT: Duration = Duration::from_millis(10000);

    let cores = 4;
    let pool = ThreadPool::new(cores + 4);

    let cfg1 = Config::new(
        SchedulerConfig::with_total_threads(cores / 2),
        RemoteConfig::listening_at(
            "127.0.0.1:20001",
            ServerConfig::default(),
            ClientConfig::default(),
        ),
    );
    let sys1 = System::new("A", "localhost", &cfg1);
    let run1 = sys1.run(&pool).unwrap();

    let cfg2 = Config::new(
        SchedulerConfig::with_total_threads(cores / 2),
        RemoteConfig::listening_at(
            "127.0.0.1:20002",
            ServerConfig::default(),
            ClientConfig::default(),
        ),
    );
    let sys2 = System::new("B", "localhost", &cfg2);
    let run2 = sys2.run(&pool).unwrap();

    run1.spawn_default::<Spy>("spy");
    run2.spawn_default::<Echo>("echo");

    let message = "Answer is 42.".to_string();
    let (tx, rx) = channel();
    let env1 = Envelope::of(Probe {
        tx,
        message: message.clone(),
        target: "echo@127.0.0.1:20002".to_string(),
    });
    run1.send("spy", env1);

    let result = rx.recv_timeout(TIMEOUT);
    run1.shutdown();
    run2.shutdown();
    if let Ok(received) = result {
        assert_eq!(received, message);
    } else {
        assert!(false, "Probe did not receive forwarded payload");
    }
}
