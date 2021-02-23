![](https://github.com/sergey-melnychuk/uppercut/workflows/Rust/badge.svg)

## Uppercut

Simple and small actor model implementation.

### Install

#### `Cargo.toml`

```toml
[dependencies]
uppercut = "0.3"
```

### Examples

#### [`examples/basic.rs`](/examples/basic.rs)

```rust
use std::sync::mpsc::{channel, Sender};

extern crate uppercut;
use uppercut::api::{AnyActor, Envelope, AnySender};
use uppercut::config::Config;
use uppercut::core::System;
use uppercut::pool::ThreadPool;
use std::thread::sleep;
use std::time::Duration;

#[derive(Debug)]
struct Message(usize, Sender<usize>);

#[derive(Default)]
struct State;

impl AnyActor for State {
    fn receive(&mut self, envelope: Envelope, sender: &mut dyn AnySender) {
        if let Some(msg) = envelope.message.downcast_ref::<Message>() {
            sender.log(&format!("received: {:?}", msg));
            msg.1.send(msg.0).unwrap();
        }
    }
}

fn main() {
    let cfg = Config::default();
    let sys = System::new("basic", "localhost", &cfg);
    let pool = ThreadPool::new(6);
    let run = sys.run(&pool).unwrap();

    run.spawn_default::<State>("state");

    let (tx, rx) = channel();
    run.send("state", Envelope::of(Message(42, tx)));

    sleep(Duration::from_secs(3));
    let result = rx.recv().unwrap();
    println!("result: {}", result);
    run.shutdown();
}
```

### More examples

- [Pi](/examples/pi.rs)
- [remote](/examples/remote.rs)
- [PAXOS](/examples/paxos.rs)
- [Gossip](/examples/gossip.rs)
