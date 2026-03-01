# Performance Optimisations

This document summarises performance-related changes and recommendations for the uppercut actor runtime and its networking layer.

## Design: Streaming / Event-Driven

The networking layer is designed for **streaming, event-driven applications**:

- **One connection = one actor.** Each TCP connection maps to exactly one actor in the system.
- **Direct receive.** Connection actors receive traffic directly from their socket (via `Work` events from the Server).
- **Multi-peer.** A connection actor can also receive messages from other actors in the system—e.g. forwarding, routing, or fan-out from other peers. Each connection is a first-class actor with its own mailbox.

This model keeps the mapping simple and location-agnostic: messages to `actor@host:port` are delivered to the connection actor for that peer, which can process them alongside any messages received over the wire.

---

## Completed Optimisations

### 1. Worker Loop: Blocking Receive (Critical)

**Problem:** Worker threads used `try_recv()` in a tight loop, causing 100% CPU usage when idle.

**Solution:** Replaced with blocking `recv()`. Workers now sleep when no events are available.

**Location:** `src/core.rs` — `worker_loop()`

**Impact:** Eliminates idle CPU burn; no meaningful latency increase since workers wake immediately when events arrive.

---

### 2. TCP_NODELAY

**Problem:** Nagle's algorithm buffers small packets, adding latency for message-passing workloads.

**Solution:** Set `TCP_NODELAY` on all TCP sockets to disable Nagle's algorithm.

**Locations:**
- `src/remote/server.rs` — on each accepted connection
- `src/remote/client.rs` — on first writable event (when connection is established; required for Windows)

**Impact:** Lower latency for small messages; beneficial for actor-style request/response patterns.

---

## Current Benchmark

The full remote example achieves **~1.7M messages/second** throughput with the above optimisations applied.

---

## Recommended Optimisations (by impact)

### High Impact

#### 1. Remove or Gate Logging in Hot Path

**Problem:** `sender.log(...)` on every send/receive allocates and adds overhead.

**Recommendation:**
- Gate logging behind `#[cfg(debug_assertions)]` or a log level
- Or use a compile-time flag / config to disable verbose logging in production

**Locations:** `src/remote/server.rs`, `src/remote/client.rs` — packet send/receive paths

---

#### 2. Reduce Allocations in Hot Path

**Problem:** Frequent allocations in the message path:
- Client: `payload.to_vec()` clones payload on every send
- Server: `Packet::from_bytes` allocates new `String`s and `Vec<u8>` per packet
- Address formatting: `format!("{}@{}", packet.from, host)` allocates per packet

**Recommendation:**
- Use `bytes::Bytes` for payloads to avoid cloning
- Reuse buffers or use compact address representation (e.g. interned strings)
- Consider `SmallVec` for small, fixed-size address strings

**Locations:** `src/remote/client.rs` (put/send), `src/remote/server.rs` (Connection::receive), `src/remote/packet.rs`

---

#### 3. Larger Read Buffers

**Problem:** Connection actors use `[0u8; 1024]` per read. Many small reads increase syscall overhead.

**Recommendation:** Use 8–64 KB buffers (configurable via `ServerConfig` / `ClientConfig`).

**Locations:** `src/remote/server.rs` (Connection), `src/remote/client.rs` (Connection), `src/config.rs`

---

#### 4. Configurable Packet Size Limit

**Problem:** `PACKET_SIZE_LIMIT` is hardcoded at 4096 + 12 bytes.

**Recommendation:** Make it configurable (e.g. in `ServerConfig`) and support larger payloads (64 KB–1 MB) for bulk transfers.

**Location:** `src/remote/server.rs`

---

### Medium Impact

#### 5. Client: Handle Missing Connections Gracefully

**Problem:** `self.connections.remove(&id).unwrap()` and `self.destinations.get(addr).unwrap()` can panic if the event token or address is unknown.

**Recommendation:** Use `if let Some(connection) = self.connections.remove(&id)` and handle the `None` case (e.g. log and continue).

**Location:** `src/remote/client.rs` — `poll()`, `put()`

---

#### 6. Poll Timeout Strategy

**Problem:** Fixed 1 ms poll timeout keeps latency low but can burn CPU when idle.

**Recommendation:** Use adaptive timeouts (e.g. longer when idle, shorter when busy) or `None` when no work is pending.

**Locations:** `src/remote/server.rs`, `src/remote/client.rs` — `poll()` calls

---

#### 7. Batch Packet Parsing (Server)

**Problem:** Connection actors parse one packet per `receive` and then break. With many packets in the buffer, this causes many actor turns.

**Recommendation:** Parse all complete packets in one `receive` and dispatch them in a batch.

**Location:** `src/remote/server.rs` — Connection::receive

---

#### 8. TCP Options: SO_REUSEPORT

**Problem:** Single listener limits scaling on multi-core machines.

**Recommendation:** Enable `SO_REUSEPORT` on the listener to allow multiple processes/threads to bind the same port.

**Location:** `src/remote/server.rs` — `listen()`

---

### Lower Impact / Structural

#### 9. Zero-Copy Payloads

**Problem:** Payloads are copied multiple times (e.g. `Vec<u8>` → `Packet` → `Envelope`).

**Recommendation:** Use `bytes::Bytes` for payloads so they can be shared without cloning.

**Locations:** `src/remote/packet.rs`, `src/api.rs` (Envelope), remote server/client

---

#### 10. Write Coalescing

**Problem:** Connection actors write as soon as `send_buf` has data. Under load, batching could reduce syscalls.

**Recommendation:** Optionally coalesce multiple packets before calling `write_all`.

**Location:** `src/remote/server.rs`, `src/remote/client.rs` — Connection write paths

---

#### 11. Connection Actor Mailbox Backpressure

**Problem:** Connection actors have unbounded mailboxes. A flood of `Work` events (from network) or messages (from other peers) can queue up.

**Recommendation:** Add backpressure (bounded mailboxes, dropping, or flow control) to avoid unbounded memory growth under load.

**Location:** `src/core.rs` — scheduler queue handling, `src/config.rs` — mailbox capacity

**Note:** The one-connection-per-actor model is intentional for streaming; each connection actor is a full actor that can receive from both network and other peers. Optimisations should preserve this model.

---

## Quick Wins Summary

| Change                         | File(s)        | Effort | Effect                    |
|--------------------------------|----------------|--------|---------------------------|
| Gate logging in hot path      | server, client | Low    | Less allocation, less I/O |
| Increase read buffer to 8 KB   | server, client, config | Low | Fewer syscalls            |
| Make packet size configurable  | server, config | Low    | Support larger messages   |
| Handle missing connection     | client         | Low    | Avoid panics              |

---

## Implementation Priority

1. **Done:** Worker busy loop fix, TCP_NODELAY
2. **Next:** Gate logging, larger buffers, configurable packet limit
3. **Then:** Allocation reduction (Bytes, address handling)
4. **Later:** Batch parsing, backpressure
