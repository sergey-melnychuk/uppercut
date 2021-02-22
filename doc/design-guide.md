### Design

#### Introduction

This document introduces basic concepts and design decisions behind standalone 
actor model implementation in Rust.

#### Core concepts

Message passing is the way we humans communicate everywhere, and it is pretty much 
the only possible way of communication that scales from individuals to nations or 
even whole World.

One of the institutions that has message passing as its core is a Post Office. 
Model and abstractions used in this document are very similar, like messages 
and envelopes.

Fundamental entities are:
- Address - logical point where Envelope can be delivered
- Envelope - a container to hold:
  - Message content
    - no restrictions apply on message content
  - Address of the sender of the Message
  - Address of the receiver of the Message
- Actor - a reactive (passive) entity that:
  - has a known Address
  - when receive an Envelope can:
    - access (change) own internal state
    - create (spawn) new Actors
    - send Envelope to a known Address
- Environment - a machinery that delivers Envelopes to Actors
  - collects (hopefully) meaningful metrics along the way

Important comments:
- Envelope makes no restrictions on Message content
  - Post Office: anyone can send anything to a known Address
  - This means that generic Actor can receive anything
  - Which in turn means that such Actor cannot be type-safe!
    - So type assertions on received Message is part of Actor's internal behavior

Each Actor holds its state internally, thus there is no direct way to access 
other's Actor internal state. This leaves message passing as the only way for 
Actors to interact with each other and with the Environment.

Actor is reactive (passive) by nature, it can only "act" when asked by the 
Environment to process the Envelope.

Actor and Address are absolutely independent and do not rely on each other. 
The Environment is responsible for binding an Actor under specific Address, 
and thus enforcing all Envelopes sent to a given Address to be received by
specific Actor, bound to that specific Address. The Environment is also 
responsible for letting the Actor know under which address it was bound.

Until an Actor is bound under specific Address, it cannot receive any messages -
thus cannot perform any action, so it is not distinguishable if such Actor even 
exists, as the only way to check it would be to send an Envelope to an Address.
As long as the Environment is responsible for such binding, Actor can only be
created inside the Environment, which makes the Environment owner of all Actors!

In order to perform any task, the Environment must provide a way to send an
Envelope to an Address from the outside - otherwise no single Actor can ever act, 
as in order to act, an Envelope must be received first. In order to be received, 
an Envelope then must be sent first!

With provided ways of (1) spawning an actor in declarative manner (as actors can
be created only inside the Environment, not outside) under given Address and (2)
sending an Envelope to a given Address, it must be possible to define initial 
configuration of the Environment: set of Actor blueprints and initial set of 
Envelopes to be sent to respective Addresses.

After that, what's left is to somehow start the Environment (actor runtime), and 
allow it to have access to required resources. If failed to start, the initial
configuration remains, thus can be restarted any required number of attempts.

#### DONE
1. Remote Actors (based on IO-bridge)
   - Address is still regular String: local-actor-address@host:port
     - Example: connection-0123@192.168.1.2:9000
   - Message must be (de)serializable to (from) bytes (serde/bincode/...)
   - Sending: send Envelope with `Vec<u8>` to Remote Actor
     - Environment detects that such an Envelope must go through IO-bridge
   - Receiving: IO-bridge reads from TCP connection
     - Environment forwards received buffer to local Actor address
   - [remote example](https://github.com/sergey-melnychuk/uppercut/blob/master/examples/remote.rs)
1. IO-bridge abstraction (based on TCP server impl)
   - TCP server listens for incoming messages from remote actors
   - TCP clients send messages to remote actors
1. Basic implementation of TCP server on top of Actors
   - TCP listener is an Actor
   - connection listeners are Actors
     - parsing bytes to HTTP/WebSocket frames
     - serializing WebSocket frames to bytes
   - [uppercut-mio-server](https://github.com/sergey-melnychuk/uppercut-lab/tree/master/uppercut-mio-server)
1. Shutdown the Environment
   - immediate:
     - stop all actors
     - drop all undelivered messages
   - graceful (wait for all tasks to complete)
     - impossible in general case, Actor Model is non-deterministic
     - **a graceful shutdown is application-specific**
1. Stop an Actor under specific Address
   - `sender.stop("addr")`
   - only local (not remote) actors can be stopped like this (as for now)
   - actor can request to stop itself using the same approach
1. Basic metrics reporting:
   - Default behaviour: dump metrics to stdout with configurable interval
1. Actor failure handling:
   - `std::panic::catch_unwind` ([doc](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html))
   - `AssertUnwindSafe` ([doc](https://doc.rust-lang.org/std/panic/struct.AssertUnwindSafe.html))
   - The `on_fail` method of `AnyActor` is fired when actor failure is detected
     - Providing custom impl of `on_fail` allows signaling/propagation of the error (if needed)
     - Generally it is hard to distinguish recoverable and non-recoverable failures
       - What is not part of a domain seems like non-recoverable failure
     - Supervision concept thus moves to application domain and client code is responsible for implementing it
   - see [fail](/examples/fail.rs) example for more details
1. Actor stop watch:
   - The `on_stop` method of `AnyActor` is called right before an actor is permanently stopped
   - This is the place where to clean up any existing resources and signal stopped processing
   - see [stop](/examples/stop.rs) example for more details
1. Raw Actors:
   - Message is just a byte buffer
     - serialize and send byte buffer
     - receive byte buffer and try deserialize
   - de-/serialization
     - no restrictions on specific impl
     - [bincode](https://github.com/servo/bincode)
1. Centralized unified Logging & Metrics:
   - Simple structured Metrics and Logging model provided
   - Logs from actors are collected and dumped to stdout (if enabled) in the event loop
   - Scheduler metrics are collected and dumped to stdout (if enabled) in the event loop
   - Custom metrics: TBD similar as Logging
1. Example implementations:
   - PAXOS (distributed consensus - simple log replication)
   - Gossip (heartbeat gossip distributed membership and failure detection)
   - TODO Distributed Hash-Table (consistent hashing, chord, etc)
   - TODO Distributed Message Queue (routing, to keep in mind: persistence)

#### TODO
1. Test-kit:
   - Allow probing for specific messages at specific Address
   - Allow accessing Actor's internal state (for tests only)
1. Persistence:
   - Message queue that can be persisted to disk
   - Allows introduction of reliability guarantees
   - Configurable backend (leveldb/rocksdb/etc)
1. Insights into Environment internals (Admin Panel)
   - Extend existing metric collection approach
   - Allow narrowing down tracing to specific Address
     - get sent/received/processed rate
     - get dump of specific messages processed by the Actor
     - get dump of Actor's current internal state
   - Consider running such actors on standalone thread-pools
     - minimize tracing overhead for the whole system
     - Actor can be easily migrated between thread-pools
   - Dashboard representing current state of the Environment?
     - standalone web-page?
     - Grafana dashboard?
   - [questdb](https://questdb.io/)

#### References

1. Hewitt, Carl; Bishop, Peter; Steiger, Richard (1973). ["A Universal Modular Actor Formalism for Artificial Intelligence". IJCAI.](
https://www.ijcai.org/Proceedings/73/Papers/027B.pdf)

1. Hewitt, Meijer and Szyperski: The Actor Model (everything you wanted to know, but were afraid to ask) [video](https://channel9.msdn.com/Shows/Going+Deep/Hewitt-Meijer-and-Szyperski-The-Actor-Model-everything-you-wanted-to-know-but-were-afraid-to-ask)

1. Wikipedia: [Actor Model](https://en.wikipedia.org/wiki/Actor_model)
