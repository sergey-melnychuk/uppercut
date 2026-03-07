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
