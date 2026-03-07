# Performance Improvements

Ranked by expected impact.

## 1. Drain all pending actions in a tight loop

`src/core.rs:386` does `recv_timeout` → processes **one** action → loops. If 100 actions are
queued, you pay 100 × `recv_timeout` call overhead. Instead:

```rust
// first action: blocking wait with timeout
let Ok(action) = actions_rx.recv_timeout(...) else { ... };
process(action);
// drain the rest without timeout
while let Ok(action) = actions_rx.try_recv() {
    process(action);
}
// now dispatch queued events in one pass
```

This collapses N loop iterations into 1 timed wait + N tight `try_recv` calls.

## 2. Cap `recv_timeout` by time-to-next-delayed-task

`src/core.rs:382-386`: the adaptive timeout goes up to 256ms. But if a delayed task is due in
5ms, you sleep 256ms before firing it. The fix:

```rust
let next_task_in = scheduler.tasks.peek()
    .map(|e| e.at.saturating_duration_since(Instant::now()))
    .unwrap_or(Duration::from_millis(max_timeout_millis));
let effective_timeout = next_task_in.min(Duration::from_millis(timeout_millis));
```

This makes `delay()` precision actually match the `delay_precision` config value.

## 3. Skip the queue when the actor is already idle

`src/core.rs:422-431`: when a `Queue` action arrives and the actor is sitting idle in
`scheduler.actors`, the code does `push_back` then immediately `pop_front` to send the same
envelope. Just send it directly:

```rust
Action::Queue { tag, envelope } if scheduler.active.contains(&tag) => {
    if let Some(actor) = scheduler.actors.remove(&tag) {
        // actor is idle — bypass queue entirely
        events_tx.send(Event::Mail { tag, actor, envelope }).unwrap();
    } else {
        scheduler.queue.get_mut(&tag).unwrap().push_back(envelope);
    }
}
```

## 4. Per-worker dedicated channels

Currently all workers share one cloned `events_rx` (`src/core.rs:541`). Even though crossbeam
is lock-free, all workers contend on the same queue head. Give each worker its own
`Sender<Event>` and have the scheduler round-robin dispatch. This also enables actor affinity
(pin actor to a worker → hot cache lines stay local).

## 5. Reduce redundant HashMap lookups

`src/core.rs:419-431` hits `scheduler.active`, `scheduler.queue`, and `scheduler.actors` with
separate lookups on the same `tag`. The `active.contains()` guard in the match arm plus the
inner `actors.remove()` is two lookups. Since `active` is a `HashSet<String>` and `actors` is
a `HashMap<String, Actor>`, you could eliminate `active` entirely — presence in `queue`
(inserted at spawn, removed at stop) serves the same purpose, saving a redundant set lookup
per message.

---

## 6. Adaptive affinity dispatch (not yet applied)

**Context:** item #4 (per-worker channels) uses tag-hash affinity — the same actor always
routes to the same worker, preserving L1/L2 cache locality. This breaks down when one actor
receives a disproportionate share of messages (hotspot), saturating its assigned worker while
others sit idle.

**Approach — least-loaded with affinity spill:**

Track two extra structures in `event_loop`:

```rust
let mut inflight: Vec<usize> = vec![0; n_workers];  // messages currently in each worker's channel
let mut assignment: HashMap<String, usize> = HashMap::new();  // tag → worker while inflight
```

Replace `worker_for` with a function that prefers the hash-assigned worker unless it is more
than one message ahead of the least-loaded worker:

```rust
fn choose_worker(tag: &str, inflight: &[usize]) -> usize {
    let n = inflight.len();
    // hash-preferred worker for affinity
    let preferred = /* hash(tag) % n */;
    let min_load = *inflight.iter().min().unwrap_or(&0);
    if inflight[preferred] > min_load + 1 {
        // spill: pick least-loaded worker
        inflight.iter().enumerate()
            .min_by_key(|(_, &c)| c)
            .map(|(i, _)| i)
            .unwrap_or(preferred)
    } else {
        preferred  // stay affine
    }
}
```

On every `Event::Mail` dispatch, increment `inflight[w]` and record `assignment[tag] = w`.
On every `Action::Return`, look up `assignment[tag]`, decrement `inflight[w]`, and remove the
entry. `Event::Stop` dispatches are **not** tracked (no `Return` follows them).

**Measured overhead vs pure hash affinity (balanced workloads):**

| Benchmark      | Pure hash affinity | Adaptive affinity | Delta  |
|----------------|--------------------|-------------------|--------|
| high_throughput | 9,713,606 ns      | 12,271,433 ns     | +26%   |
| parallel_actors | 4,320,210 ns      | 5,885,664 ns      | +36%   |
| delay_latency   | 1,289,278 ns      | 1,291,000 ns      | ~0%    |

The overhead comes from `HashMap::insert/remove` per dispatch and an `O(n_workers)` scan in
`choose_worker`. For balanced workloads (equal message rates across actors) the spill path
never fires, so the cost is pure overhead with no benefit.

**When to apply:** only worth enabling if profiling shows a specific actor pinned to one
worker while other workers are measurably idle. A cheaper mitigation for mild imbalance is
simply increasing `actor_worker_threads` so the hash distributes over more buckets.

**Possible optimisation before applying:** replace `HashMap<String, usize>` with a `usize`
field embedded directly in a per-actor wrapper struct (eliminates the hash map), and maintain
a running `min_inflight` variable updated on every increment/decrement (eliminates the
`O(n_workers)` scan). That would reduce the overhead to a handful of integer operations per
dispatch.

---

## 7. Watchdog thread — heartbeat-based blocking detection (not yet applied)

Actors are CPU-only. A blocking call inside `receive()` stalls the entire worker thread,
preventing all actors hashed to it from making progress. The current `catch_unwind` only
catches panics, not hangs.

**Approach — atomic heartbeat + watchdog:**

Share one `AtomicU64` per worker between `worker_loop` and a dedicated watchdog thread.
Each worker stamps the current time when it begins processing a message and clears it when
done (0 = idle):

```rust
// Shared: Arc<Vec<AtomicU64>>, one entry per worker thread.

// In worker_loop, wrapping actor.receive():
dispatch_at[w].store(now_ms(), Ordering::Release);
let result = catch_unwind(AssertUnwindSafe(|| actor.receive(envelope, &mut sender)));
dispatch_at[w].store(0, Ordering::Release);
```

The watchdog thread runs on a `extra_worker_threads` slot and polls on a configurable
interval (e.g. every 100 ms):

```rust
loop {
    thread::sleep(check_interval);
    let now = now_ms();
    for (w, slot) in dispatch_at.iter().enumerate() {
        let started = slot.load(Ordering::Acquire);
        if started != 0 && now.saturating_sub(started) > deadline_ms {
            // worker w has been stuck for > deadline — log / emit metric
        }
    }
}
```

This is zero-overhead on the hot path when workers are healthy (one store before, one store
after each `receive()`). The watchdog itself is off the critical path entirely.

**What it detects:** any call inside `receive()` that does not return within the deadline —
`thread::sleep`, blocking `read`/`write`, `Mutex::lock` on a contended lock, infinite loops.

**What it cannot do:** interrupt the stuck thread. Rust provides no safe forced-kill for
threads. The watchdog can log, emit a metric, or ultimately call `process::abort()` if the
deadline is badly exceeded and progress is permanently lost.

---

## 8. OS thread state detection — distinguish blocking cause (Linux only, not yet applied)

Combines with item #7 to explain *why* a worker is stuck, not just *that* it is stuck.

On Linux every thread exposes its kernel state in `/proc/self/task/<tid>/stat`. The third
field (after the process name) is a single character:

| State | Meaning | Verdict for a CPU actor |
|-------|---------|------------------------|
| `R`   | running or runnable | legitimate heavy computation |
| `S`   | interruptible sleep (futex, condvar, channel `recv`) | blocked on a sync primitive — contract violation |
| `D`   | uninterruptible disk wait | blocking I/O in actor — worst case, cannot be killed until the kernel call returns |

**Implementation:**

Capture each worker's OS thread ID at spawn time using `libc::syscall(SYS_gettid)` and store
it alongside the heartbeat slot:

```rust
// At worker spawn (inside the thread):
let tid = unsafe { libc::syscall(libc::SYS_gettid) as u32 };
tid_slot[w].store(tid, Ordering::Release);
```

In the watchdog, once a heartbeat timeout fires, read the thread state:

```rust
fn thread_state(tid: u32) -> Option<char> {
    let path = format!("/proc/self/task/{}/stat", tid);
    let stat = std::fs::read_to_string(path).ok()?;
    // comm field is wrapped in parens and may contain spaces; find the closing paren
    let after_comm = stat.rfind(')')? + 2;
    stat[after_comm..].chars().next()
}
```

Combined signal from items #7 + #8:

```
heartbeat timed out + state == 'S'  →  blocked on mutex/channel/condvar (classic mistake)
heartbeat timed out + state == 'D'  →  blocking I/O (e.g. file read, connect) — most dangerous
heartbeat timed out + state == 'R'  →  long CPU computation (may be intentional)
```

`D`-state detection is the most valuable: it definitively identifies blocking I/O inside an
actor even when the thread is otherwise unresponsive.

**Dependencies:** requires `libc` (already an indirect dependency via `mio`/`core_affinity`);
the `/proc` read is a virtual filesystem access with no disk I/O.

**Portability:** Linux only. On macOS the equivalent is `proc_pidinfo` via `libproc`, which
requires an additional dependency. Gate behind `#[cfg(target_os = "linux")]`.
