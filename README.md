![](https://github.com/sergey-melnychuk/uppercut/workflows/Rust/badge.svg)

## Uppercut

Simple and small actor model implementation.

### Install

#### `Cargo.toml`

```toml
[dependencies]
uppercut = "0.4"
```

### Example

#### [`hello.rs`](/examples/hello.rs)

```shell
$ cargo run --example hello
[...]
result: 42
```

#### [`pi.rs`](/examples/pi.rs)

```shell
$ cargo run --release --example pi
[...]
Submitting 10000 workers making 100000 throws each.
Pi estimate: 3.141561988 (in 5 seconds)
```

### More examples

- [remote](/examples/remote.rs)
- [Gossip](/examples/gossip.rs)
- [PAXOS](/examples/paxos.rs)
