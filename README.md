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
    // in uppercut actor is not type-safe by definition: anyone can send anything to anyone
    fn receive(&mut self, envelope: Envelope, _sender: &mut dyn AnySender) {
        // down-casting is required to do something meaningful with the envelope
        if let Some(msg) = envelope.message.downcast_ref::<Message>() {
            println!("Actor 'State' received a message: {}", msg.0);
            msg.1.send(msg.0).unwrap();
        }
    }
}

fn main() {
    // default config needs 6 threads: 1 main, 4 workers and 1 utility
    let pool = ThreadPool::new(6);

    let sys = System::default();
    let run = sys.run(&pool).unwrap();

    // spawn actor in the default state at address "x"
    run.spawn_default::<State>("x");

    let (tx, rx) = channel();
    // send message to actor ('from' address is an empty string)
    run.send("x", Envelope::of(Message(42, tx), ""));

    rx.recv().unwrap();
    run.shutdown();
}
```
