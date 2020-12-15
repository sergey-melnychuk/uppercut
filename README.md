![](https://github.com/sergey-melnychuk/uppercut/workflows/Rust/badge.svg)

## Uppercut

Simple and small actor model implementation.

### Example

#### `Cargo.toml`

```toml
[dependencies]
uppercut = "0.2"
```

#### `src/main.rs`

```rust
use std::sync::mpsc::{channel, Sender};

extern crate uppercut;
use uppercut::api::{AnyActor, Envelope, AnySender};
use uppercut::core::System;
use uppercut::pool::ThreadPool;

struct Message(usize, Sender<usize>);

#[derive(Default)]
struct State;

impl AnyActor for State {
    fn receive(&mut self, envelope: Envelope, _sender: &mut dyn AnySender) {
        if let Some(msg) = envelope.message.downcast_ref::<Message>() {
            println!("Actor 'State' received a message: {}", msg.0);
            msg.1.send(msg.0).unwrap();
        }
    }
}

fn main() {
    let sys = System::default();
    let pool = ThreadPool::new(6);
    let run = sys.run(&pool).unwrap();

    run.spawn_default::<State>("x");

    let (tx, rx) = channel();
    run.send("x", Envelope::of(Message(42, tx)));

    rx.recv().unwrap();
    run.shutdown();
}
```
