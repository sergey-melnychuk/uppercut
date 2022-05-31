use std::fmt::Debug;
use std::ops::Add;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::time::Duration;

use uppercut::api::{AnyActor, AnySender, Envelope};
use uppercut::config::Config;
use uppercut::core::{Run, System};
use uppercut::pool::ThreadPool;

const ANSWER: usize = 42;

#[derive(Debug)]
struct Init(Sender<usize>);

struct Test(usize);

impl AnyActor for Test {
    fn receive(&mut self, envelope: Envelope, _sender: &mut dyn AnySender) {
        if let Some(message) = envelope.message.downcast_ref::<Init>() {
            message.0.send(self.0).unwrap();
        }
    }
}

struct Proxy {
    target: String,
}

impl AnyActor for Proxy {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        sender.send(&self.target, envelope);
    }
}

#[derive(Debug)]
struct Counter(usize, Sender<usize>);

#[derive(Debug)]
enum CounterProtocol {
    Inc,
    Get,
}

impl AnyActor for Counter {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(p) = envelope.message.downcast_ref::<CounterProtocol>() {
            match p {
                CounterProtocol::Inc => {
                    self.0 += 1;
                    self.1.send(self.0).unwrap();
                }
                CounterProtocol::Get => {
                    let env = Envelope::of(CounterProtocol::Inc);
                    sender.send(&sender.myself(), env);
                }
            }
        }
    }
}

fn with_run<T: Eq + Debug, E, F: FnOnce(&Run) -> Result<T, E>>(expected: T, f: F) -> Result<(), E> {
    let cfg = Config::default();
    let pool = ThreadPool::for_config(&cfg);
    let sys = System::new("test", "localhost", &cfg);
    let run = sys.run(&pool).unwrap();
    let got = f(&run);
    run.shutdown();
    let actual = got?;
    assert_eq!(actual, expected);
    Ok(())
}

const TIMEOUT: Duration = Duration::from_millis(500);

#[test]
fn sent_message_received() -> Result<(), RecvTimeoutError> {
    with_run(ANSWER, |run| {
        run.spawn("test", || Box::new(Test(ANSWER)));

        let (tx, rx) = channel();
        let env = Envelope::of(Init(tx));
        run.send("test", env);

        rx.recv_timeout(TIMEOUT)
    })
}

#[test]
fn forwarded_message_received() -> Result<(), RecvTimeoutError> {
    with_run(ANSWER, |run| {
        run.spawn("test", || Box::new(Test(ANSWER)));
        run.spawn("proxy", || {
            Box::new(Proxy {
                target: "test".to_string(),
            })
        });

        let (tx, rx) = channel();
        let env = Envelope::of(Init(tx));
        run.send("proxy", env);

        rx.recv_timeout(TIMEOUT)
    })
}

#[test]
fn delayed_message_received() -> Result<(), RecvTimeoutError> {
    with_run(ANSWER, |run| {
        run.spawn("test", || Box::new(Test(ANSWER)));

        let (tx, rx) = channel();
        let env = Envelope::of(Init(tx));

        const DELAY: Duration = Duration::from_millis(100);
        run.delay("test", env, DELAY);

        rx.recv_timeout(TIMEOUT.add(DELAY))
    })
}

#[test]
fn own_message_received() -> Result<(), RecvTimeoutError> {
    with_run(ANSWER + 1, |run| {
        let (tx, rx) = channel();
        run.spawn("test", || Box::new(Counter(ANSWER, tx)));

        let env = Envelope::of(CounterProtocol::Get);
        run.send("test", env);

        rx.recv_timeout(TIMEOUT)
    })
}

struct Replier(Sender<usize>);

impl AnyActor for Replier {
    fn receive(&mut self, envelope: Envelope, _sender: &mut dyn AnySender) {
        if let Some(n) = envelope.message.downcast_ref::<usize>() {
            self.0.send(*n).unwrap();
        }
    }
}

#[test]
fn message_order_perceived() -> Result<(), RecvTimeoutError> {
    let n = 10;
    let seq: Vec<usize> = (1..=n).into_iter().collect();
    with_run(seq, |run| {
        let (tx, rx) = channel();
        run.spawn("test", || Box::new(Replier(tx)));

        for x in 1..=n {
            let e = Envelope::of(x);
            run.send("test", e);
        }

        let mut vec: Vec<usize> = Vec::with_capacity(n);
        for _ in 0..n {
            let x = rx.recv_timeout(TIMEOUT)?;
            vec.push(x);
        }
        Ok(vec)
    })
}

struct Fan(String, usize);

impl AnyActor for Fan {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(n) = envelope.message.downcast_ref::<usize>() {
            for x in 0..*n {
                let response = Envelope::of((x, self.1));
                sender.delay(&self.0, response, Duration::from_millis(100));
                self.1 += 1;
            }
        }
    }
}

struct Echo(Sender<(usize, usize)>);

impl AnyActor for Echo {
    fn receive(&mut self, envelope: Envelope, _sender: &mut dyn AnySender) {
        if let Some(pair) = envelope.message.downcast_ref::<(usize, usize)>() {
            self.0.send(pair.to_owned()).unwrap();
        }
    }
}

#[test]
fn delayed_messages_ordering() -> Result<(), RecvTimeoutError> {
    const N: usize = 3;
    let seq: Vec<usize> = (0..N).into_iter().collect();
    let expected: Vec<(usize, usize)> = seq.iter().map(|x| (*x, *x)).collect();
    with_run(expected, |run| {
        let (tx, rx) = channel();
        run.spawn("echo", || Box::new(Echo(tx)));
        run.spawn("seq", || Box::new(Fan("echo".to_string(), 0)));

        run.send("seq", Envelope::of(N));
        let mut actual = Vec::with_capacity(N);
        for _ in 0..N {
            let x = rx.recv_timeout(TIMEOUT)?;
            actual.push(x);
        }

        Ok(actual)
    })
}
