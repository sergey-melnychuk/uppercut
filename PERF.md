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
