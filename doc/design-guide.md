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
- Actor - a reactive (passive) entity that can:
  - have a known Address
  - when receive an Envelope:
    - access or change own internal state
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
configuration remains static, thus can be restarted any required number of attempts.

#### Open questions

1. Shutting individual Actors or the whole Environment down:
   - it is required to find a way to terminate an Actor to avoid resource leak
   - same thing with the Environment - there must be a trigger to shut it down
1. Interacting with started & running Environment:
   - is it really needed?
   - for what use cases?
1. Collecting internal telemetry about the Environment:
   - metrics: messages throughput per actor, system-wise
   - events: detect congestion? detect infinite loop?
   - how external watcher can be sure the Environment is making progress?
     - watcher must be able to send and receive messages
     - watcher is an actor itself?
1. Getting something back from the Environment:
   - subscribe a callback to specific events?
   - concept of a value wrapped with `Future`? type-safety?
   - simple `Arc<Mutex<T>>` as an external boundary?
     - synchronization required to actually produce value
   - `Stream` of events as an output channel from the Environment?
     - half-actor / half-iterable?
   - `Channel` for Actor to publish result to (see 'basis' example).
1. When a new Actor is spawned under the taken Address:
   - Not Environment's problem - but User's one
   - Simply ignore an attempt to spawn an Actor under taken Address
   - Any guarantees must be part of the application level protocol:
     - create Actor 'child'
     - send 'ping' to 'child'
     - receive 'pong' from 'child'
     - 'child' was spawned successfully

#### Implementation tasks

1. TODO Actor failure handling
   - `std::panic::catch_unwind` ([doc](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)) + `AssertUnwindSafe` ([doc](https://doc.rust-lang.org/std/panic/struct.AssertUnwindSafe.html)) 
   - if recoverable - recovery to stable state (simply ignore and carry on)
     - recoverable == can carry on performing the task
     - occurred in 100% owned code (not library, not RPC nor 3rd party)
     - does not depend on external resources (file/socket descriptors, etc)
     - such failure must be part of the application domain
     - e.g. failed to parse JSON from given string
   - if non-recoverable - propagate
     - non-recoverable == can't make any more progress due to the failure
     - e.g. failed to bind a listener to specific port number
     - propagate where? to "parent"/"supervisor"? no such concepts exist here yet!
     - TODO this section needs extra attention
1. DONE Shutdown an Actor under specific Address
     - send a message asking the Actor to stop
     - ask Scheduler to mark Address as free/unoccupied
1. DONE Shutdown the Environment
   - DONE immediate:
     - stop all actors
     - drop all undelivered messages
   - graceful (wait for all tasks to complete)?
     - impossible in general case, Actor Model is non-deterministic
     - a graceful shutdown is application-specific
1. TODO Basic implementation of TCP/HTTP/WebSocket server on top of Actors
   - TCP listener is an Actor
   - connection listeners are Actors
     - parsing bytes to HTTP/WebSocket frames
     - serializing WebSocket frames to bytes
1. TODO IO-bridge abstraction (based on TCP/... server impl)
   - IO-bridge runs on separate thread-pool (IO-bound)
   - TCP server listens for incoming messages from remote actors
   - TCP clients send messages to remote actors
   - filesystem access
     - configuration
     - logging
     - persistence
1. TODO Remote Actors (based on IO-bridge)
   - Address is still regular String: local-actor-address@host:port
     - Example: connection-0123@192.168.1.2:9000
   - Message must be (de)serializable to (from) bytes (serde/bincode/...)
   - Sending: send Envelope with `Vec<u8>` to Remote Actor
     - Environment detects that such an Envelope must go through IO-bridge
   - Receiving: IO-bridge reads from TCP connection
     - Environment forwards received buffer to local Actor address
1. TODO Define beneficial use-cases and provide example implementations
   - distributed hash-table
   - distributed lock service
     - PAXOS and friends
   - large-scale stream processing
     - persistence?
     - idempotence?
     - fault-tolerance? (e.g. spark-style checkpoints)
1. TODO Raw Actors:
   - Message is just a byte buffer
     - serialize and send byte buffer
     - receive byte buffer and try deserialize
   - Such Raw Actor can be used as connection handler in mio-based event loop
     - yet incoming message can be checked if it is `&[u8]`
     - so no strict distinction between Raw Actor and just Actor
   - (de)serialization
     - `fn(&[u8]) -> Option<T>`
     - `fn(T) -> Vec<u8>` 
     - or `fn(T, &mut [u8]) -> Result<usize>` (potentially no allocation)
     - [bincode](https://github.com/servo/bincode)
1. TODO Metrics reporting:
   - Default behaviour: dump metrics to stdout with configurable interval
   - Allow reporting metrics to graphite/prometheus
1. TODO Insights into Environment internals
   - Extend existing metric collection approach
   - Allow narrowing down tracing to specific Address
     - get sent/received/processed rate
     - get dump of specific messages processed by the Actor
     - get dump of Actor's current internal state
   - Consider running such actors on standalone thread-pools
     - minimize tracing overhead for the whole system
     - Actor can be easily migrated between thread-pools
   - Dashboard representing current state of the Environment?
     - Standalone web-page?
     - Grafana dashboard?
1. TODO Test-kit:
   - Allow probing for specific messages at specific Address
   - Allow accessing Actor's internal state (for tests only)

#### References

1. Hewitt, Carl; Bishop, Peter; Steiger, Richard (1973). ["A Universal Modular Actor Formalism for Artificial Intelligence". IJCAI.](
https://www.ijcai.org/Proceedings/73/Papers/027B.pdf)

1. Wikipedia: [Actor Model](https://en.wikipedia.org/wiki/Actor_model)
