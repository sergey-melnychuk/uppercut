use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::ops::Add;
use std::panic::{self, AssertUnwindSafe};
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::{unbounded, Receiver, SendError, Sender};

use crate::api::{Actor, AnyActor, AnySender, Envelope};
use crate::config::{Config, SchedulerConfig};
use crate::error::Error;
use crate::monitor::{LoggerEntry, Meta, MetricEntry, SchedulerMetrics};
#[cfg(feature = "actor-stats")]
use crate::monitor::ActorStats;
use crate::pool::{Runnable, ThreadPool};

use crate::remote::client::{self, Client};
use crate::remote::server::{self, Server};

const CLIENT: &str = "$CLIENT";
const SERVER: &str = "$SERVER";

impl AnySender for Local {
    fn me(&self) -> &str {
        &self.tag
    }

    fn send(&self, address: &str, mut envelope: Envelope) {
        let tag = adjust_remote_address(address, &mut envelope).to_string();
        let action = Action::Queue { tag, envelope };
        self.tx.send(action).unwrap();
    }

    fn spawn(&self, address: &str, f: Box<dyn FnOnce() -> Actor>) {
        let action = Action::Spawn {
            tag: address.to_string(),
            actor: f(),
        };
        self.tx.send(action).unwrap();
    }

    fn delay(&self, address: &str, mut envelope: Envelope, duration: Duration) {
        let at = Instant::now().add(duration);
        let tag = adjust_remote_address(address, &mut envelope).to_string();
        let entry = Entry { at, tag, envelope };
        let action = Action::Delay { entry };
        self.tx.send(action).unwrap();
    }

    fn stop(&self, address: &str) {
        let action = Action::Stop {
            tag: address.to_string(),
        };
        self.tx.send(action).unwrap();
    }

    fn log(&mut self, message: &str) {
        self.logs.push((self.now(), message.to_string()));
    }

    fn metric(&mut self, name: &str, value: f64) {
        let now = self.now();
        self.metrics
            .entry(name.to_string())
            .or_default()
            .push((now, value));
    }

    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

impl Local {
    fn drain(&mut self, tx: &Sender<Action>) -> Result<(), SendError<Action>> {
        if !self.logs.is_empty() {
            let action = Action::Logs {
                tag: self.tag.clone(),
                logs: self.logs.to_owned(),
            };
            tx.send(action)?;
            self.logs.clear();
        }
        if !self.metrics.is_empty() {
            let action = Action::Metrics {
                map: self.metrics.to_owned(),
            };
            tx.send(action)?;
            self.metrics.clear();
        }
        Ok(())
    }
}

struct Local {
    tx: Sender<Action>,
    tag: String,
    logs: Vec<(SystemTime, String)>,
    metrics: HashMap<String, Vec<(SystemTime, f64)>>,
}

impl Local {
    fn new(tx: Sender<Action>) -> Self {
        Self {
            tx,
            tag: Default::default(),
            logs: Default::default(),
            metrics: Default::default(),
        }
    }
}

struct Scheduler {
    config: SchedulerConfig,
    actors: HashMap<String, Actor>,
    queue: HashMap<String, VecDeque<Envelope>>,
    tasks: BinaryHeap<Entry>,
    #[cfg(feature = "actor-stats")]
    stats: HashMap<String, ActorStats>,
}

impl Scheduler {
    fn with_config(config: &SchedulerConfig) -> Scheduler {
        Scheduler {
            config: config.clone(),
            actors: HashMap::default(),
            queue: HashMap::default(),
            tasks: BinaryHeap::default(),
            #[cfg(feature = "actor-stats")]
            stats: HashMap::default(),
        }
    }
}

// received by Worker threads
enum Event {
    Mail {
        tag: String,
        actor: Actor,
        envelope: Envelope,
    },
    Stop {
        tag: String,
        actor: Actor,
    },
    Shutdown,
}

// received by the Scheduler thread
enum Action {
    Return {
        tag: String,
        actor: Actor,
        ok: bool,
        elapsed_us: u64,
    },
    Spawn {
        tag: String,
        actor: Actor,
    },
    Queue {
        tag: String,
        envelope: Envelope,
    },
    Delay {
        entry: Entry,
    },
    Stop {
        tag: String,
    },
    Logs {
        tag: String,
        logs: Vec<(SystemTime, String)>,
    },
    Metrics {
        map: HashMap<String, Vec<(SystemTime, f64)>>,
    },
    Shutdown,
}

struct Entry {
    at: Instant,
    tag: String,
    envelope: Envelope,
}

impl Eq for Entry {}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at
    }
}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        // reverse ordering for min-heap
        self.at.cmp(&other.at).reverse()
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct Runtime<'a> {
    name: String,
    host: String,
    pool: &'a ThreadPool,
    config: Config,
}

impl<'a> Runtime<'a> {
    fn new(name: String, host: String, pool: &'a ThreadPool, config: Config) -> Runtime<'a> {
        Runtime {
            name,
            host,
            pool,
            config,
        }
    }

    fn start(self) -> Result<Run<'a>, Error> {
        let (pool, config) = (self.pool, self.config);
        let actions = unbounded();
        let sender = actions.0.clone();

        start_actor_runtime(
            self.name,
            self.host,
            pool,
            config.scheduler,
            actions,
        );
        let run = Run { sender, pool };

        if config.remote.enabled {
            let server = Server::listen(&config.remote.listening, &config.remote.server)?;
            let port = server.port();
            run.spawn(SERVER, move || Box::new(server));
            run.send(SERVER, Envelope::of(server::Loop));
            let client = Client::new(port, &config.remote.client);
            run.spawn(CLIENT, move || Box::new(client));
            run.send(CLIENT, Envelope::of(client::Loop));
        }

        Ok(run)
    }
}

#[derive(Default)]
pub struct System {
    name: String,
    host: String,
    config: Config,
}

impl System {
    pub fn new(name: &str, host: &str, config: &Config) -> System {
        System {
            name: name.to_string(),
            host: host.to_string(),
            config: config.clone(),
        }
    }

    pub fn run(self, pool: &ThreadPool) -> Result<Run<'_>, Error> {
        if pool.size() < self.config.scheduler.total_threads_required() {
            Err(Error::ThreadPoolTooSmall {
                required: self.config.scheduler.total_threads_required(),
                available: pool.size(),
            })
        } else {
            let runtime = Runtime::new(self.name, self.host, pool, self.config);
            Ok(runtime.start()?)
        }
    }
}

pub struct Run<'a> {
    pool: &'a ThreadPool,
    sender: Sender<Action>,
}

impl<'a> Run<'a> {
    pub fn send(&self, address: &str, mut envelope: Envelope) {
        let tag = adjust_remote_address(address, &mut envelope).to_string();
        let action = Action::Queue { tag, envelope };
        self.sender.send(action).unwrap();
    }

    pub fn spawn<F: FnOnce() -> Actor>(&self, address: &str, f: F) {
        let action = Action::Spawn {
            tag: address.to_string(),
            actor: f(),
        };
        self.sender.send(action).unwrap();
    }

    pub fn spawn_default<T: 'static + AnyActor + Send + Default>(&self, address: &str) {
        let action = Action::Spawn {
            tag: address.to_string(),
            actor: Box::<T>::default(),
        };
        self.sender.send(action).unwrap();
    }

    pub fn delay(&self, address: &str, mut envelope: Envelope, duration: Duration) {
        let at = Instant::now().add(duration);
        let tag = adjust_remote_address(address, &mut envelope).to_string();
        let entry = Entry { at, tag, envelope };
        let action = Action::Delay { entry };
        self.sender.send(action).unwrap();
    }

    pub fn stop(&self, address: &str) {
        let action = Action::Stop {
            tag: address.to_string(),
        };
        self.sender.send(action).unwrap();
    }

    pub fn shutdown(self) {
        let action = Action::Shutdown;
        let _ = self.sender.send(action);
    }

    pub fn submit<F: FnOnce() + Send + 'static>(&self, f: F) {
        self.pool.submit(f);
    }
}

fn worker_loop(tx: Sender<Action>, rx: Receiver<Event>) {
    let mut sender = Local::new(tx.clone());
    loop {
        let event = match rx.recv() {
            Ok(e) => e,
            Err(_) => break,
        };
        match event {
            Event::Mail {
                tag,
                mut actor,
                envelope,
            } => {
                sender.tag = tag.clone();
                #[cfg(feature = "actor-stats")]
                let t0 = Instant::now();
                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    actor.receive(envelope, &mut sender);
                }));
                #[cfg(feature = "actor-stats")]
                let elapsed_us = t0.elapsed().as_micros() as u64;
                #[cfg(not(feature = "actor-stats"))]
                let elapsed_us = 0u64;
                let ok = result.is_ok();
                if !ok {
                    actor.on_fail(result.err().unwrap(), &mut sender);
                }
                let sent = tx.send(Action::Return { tag, actor, ok, elapsed_us });
                if sent.is_err() {
                    break;
                }
            }
            Event::Stop { tag, actor } => {
                sender.tag = tag;
                actor.on_stop(&mut sender);
            }
            Event::Shutdown => break,
        }
        sender.drain(&tx).unwrap();
    }
}

fn worker_for(tag: &str, n: usize) -> usize {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    tag.hash(&mut h);
    (h.finish() as usize) % n
}

fn event_loop(
    actions_rx: Receiver<Action>,
    actions_tx: Sender<Action>,
    events_txs: Vec<Sender<Event>>,
    mut scheduler: Scheduler,
    background: impl Fn(Runnable),
    name: String,
    host: String,
) {
    let n_workers = events_txs.len();
    let mut scheduler_metrics = SchedulerMetrics::named(name.clone());
    let mut start = Instant::now();
    let mut logs = Vec::with_capacity(1024);
    let mut metrics = HashMap::with_capacity(1024);

    let min_timeout_millis: u64 = 1;
    let max_timeout_millis: u64 = 256;
    let mut timeout_millis = max_timeout_millis;
    'main: loop {
        let effective_timeout = scheduler
            .tasks
            .peek()
            .map(|e| e.at.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_millis(timeout_millis))
            .min(Duration::from_millis(timeout_millis));
        let received = actions_rx.recv_timeout(effective_timeout);
        if let Ok(action) = received {
            timeout_millis = std::cmp::max(min_timeout_millis, timeout_millis / 2);
            let mut pending = Some(action);
            while let Some(action) = pending {
                scheduler_metrics.hit += 1;
                match action {
                    Action::Return { tag, actor, ok, elapsed_us: _elapsed_us } if scheduler.queue.contains_key(&tag) => {
                        #[cfg(feature = "actor-stats")]
                        if let Some(s) = scheduler.stats.get_mut(&tag) {
                            s.record_elapsed(_elapsed_us);
                        }
                        if !ok {
                            scheduler_metrics.failures += 1;
                            scheduler.queue.remove(&tag);
                        } else {
                            scheduler_metrics.returns += 1;

                            if let Some(envelope) = scheduler.queue.get_mut(&tag).unwrap().pop_front() {
                                let w = worker_for(&tag, n_workers);
                                let event = Event::Mail { tag, actor, envelope };
                                events_txs[w].send(event).unwrap();
                            } else {
                                scheduler.actors.insert(tag, actor);
                            }
                        }
                    }
                    Action::Return { tag, actor, ok, .. } => {
                        // Returned actor was stopped before (removed from active set).
                        scheduler.queue.remove(&tag);
                        if ok {
                            let w = worker_for(&tag, n_workers);
                            let event = Event::Stop { tag, actor };
                            events_txs[w].send(event).unwrap();
                        }
                    }
                    Action::Queue { tag, envelope } if scheduler.queue.contains_key(&tag) => {
                        scheduler_metrics.queues += 1;
                        scheduler_metrics.messages += 1;
                        if let Some(actor) = scheduler.actors.remove(&tag) {
                            // Actor is idle — dispatch directly without touching the queue.
                            let w = worker_for(&tag, n_workers);
                            let event = Event::Mail { tag, actor, envelope };
                            events_txs[w].send(event).unwrap();
                        } else {
                            scheduler.queue.get_mut(&tag).unwrap().push_back(envelope);
                            #[cfg(feature = "actor-stats")]
                            if let Some(s) = scheduler.stats.get_mut(&tag) {
                                let depth = scheduler.queue.get(&tag).unwrap().len();
                                if depth > s.mailbox_depth_max {
                                    s.mailbox_depth_max = depth;
                                }
                            }
                        }
                    }
                    Action::Spawn { tag, actor } if !scheduler.queue.contains_key(&tag) => {
                        scheduler_metrics.spawns += 1;
                        scheduler.actors.insert(tag.clone(), actor);
                        scheduler.queue.insert(
                            tag.clone(),
                            VecDeque::with_capacity(scheduler.config.default_mailbox_capacity),
                        );
                        #[cfg(feature = "actor-stats")]
                        scheduler.stats.insert(tag.clone(), ActorStats::new(tag));
                    }
                    Action::Delay { entry } => {
                        scheduler_metrics.delays += 1;
                        scheduler.tasks.push(entry);
                    }
                    Action::Stop { tag } if scheduler.queue.contains_key(&tag) => {
                        scheduler_metrics.stops += 1;
                        scheduler.queue.remove(&tag);
                        #[cfg(feature = "actor-stats")]
                        scheduler.stats.remove(&tag);
                        if scheduler.actors.contains_key(&tag) {
                            let actor = scheduler.actors.remove(&tag).unwrap();
                            let w = worker_for(&tag, n_workers);
                            let event = Event::Stop { tag, actor };
                            events_txs[w].send(event).unwrap();
                        }
                    }
                    Action::Logs { tag, logs: entries } => {
                        logs.push((tag, entries));
                    }
                    Action::Metrics { map } => {
                        for (name, mut entries) in map {
                            metrics
                                .entry(name)
                                .or_insert_with(Vec::default)
                                .append(&mut entries);
                        }
                    }
                    Action::Shutdown => break 'main,
                    _ => {
                        scheduler_metrics.drops += 1;
                    }
                }
                pending = actions_rx.try_recv().ok();
            }
        } else {
            timeout_millis = std::cmp::min(timeout_millis * 2, max_timeout_millis);
            scheduler_metrics.miss += 1;
        }

        let now = Instant::now().add(scheduler.config.delay_precision / 2);
        while scheduler
            .tasks
            .peek()
            .map(|e| e.at <= now)
            .unwrap_or_default()
        {
            if let Some(Entry { tag, envelope, .. }) = scheduler.tasks.pop() {
                let action = Action::Queue { tag, envelope };
                actions_tx.send(action).unwrap();
            }
        }

        scheduler_metrics.ticks += 1;
        if start.elapsed() >= scheduler.config.metric_reporting_interval {
            let now = SystemTime::now();
            scheduler_metrics.at = now
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            scheduler_metrics.actors = scheduler.queue.len() as u64;

            if scheduler.config.metric_reporting_enabled {
                #[cfg(feature = "actor-stats")]
                {
                    let stats_snapshot: Vec<ActorStats> =
                        scheduler.stats.values().cloned().collect();
                    for s in scheduler.stats.values_mut() {
                        s.reset();
                    }
                    report_actor_stats(&background, &name, &host, stats_snapshot);
                }
                report_metrics(
                    &background,
                    &name,
                    &host,
                    scheduler_metrics.clone(),
                    metrics,
                );
                scheduler_metrics.reset();
                metrics = HashMap::with_capacity(1024);
            }

            if scheduler.config.logging_enabled {
                report_logs(&background, &name, &host, logs);
                logs = Vec::with_capacity(1024);
            }

            start = Instant::now();
        }

        if scheduler.config.eager_shutdown_enabled && scheduler.queue.is_empty() {
            // Shutdown if there are no running actors (no progress can be made in such system).
            break 'main;
        }
    }
    for tx in &events_txs {
        tx.send(Event::Shutdown).unwrap();
    }
}

fn start_actor_runtime(
    name: String,
    host: String,
    pool: &ThreadPool,
    scheduler_config: SchedulerConfig,
    actions: (Sender<Action>, Receiver<Action>),
) {
    let (actions_tx, actions_rx) = actions;

    let scheduler = Scheduler::with_config(&scheduler_config);

    let thread_count = scheduler.config.actor_worker_threads;
    let mut events_txs = Vec::with_capacity(thread_count);
    for _ in 0..thread_count {
        let (events_tx, events_rx) = unbounded();
        events_txs.push(events_tx);
        let tx = actions_tx.clone();
        pool.submit(move || {
            worker_loop(tx, events_rx);
        });
    }

    let background = pool.link();
    pool.submit(move || {
        event_loop(
            actions_rx.clone(),
            actions_tx,
            events_txs,
            scheduler,
            background,
            name,
            host,
        );
        while actions_rx.recv().is_ok() {
            // Drain remaining actions sent from worker threads while they
            // (worker threads) are being shut down to avoid race condition
            // caused by actions_rx being dropped and worker threads that are
            // still running keep failing to send actions into closed channel.
        }
    });
}

fn adjust_remote_address<'a>(address: &'a str, envelope: &'a mut Envelope) -> &'a str {
    if address.contains('@') {
        envelope.to = address.to_string();
        return CLIENT;
    }
    address
}

fn report_logs(
    background: &impl Fn(Runnable),
    app: &str,
    host: &str,
    logs: Vec<(String, Vec<(SystemTime, String)>)>,
) {
    let app = app.to_string();
    let host = host.to_string();
    background(Box::new(move || {
        for (tag, stm) in logs {
            for (st, msg) in stm {
                let at = st
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                let log = LoggerEntry {
                    at,
                    meta: Meta {
                        host: host.clone(),
                        app: app.clone(),
                        tag: tag.clone(),
                    },
                    log: msg,
                };
                println!("{:?}", log);
            }
        }
    }));
}

#[cfg(feature = "actor-stats")]
fn report_actor_stats(
    background: &impl Fn(Runnable),
    app: &str,
    host: &str,
    stats: Vec<ActorStats>,
) {
    let app = app.to_string();
    let host = host.to_string();
    background(Box::new(move || {
        for s in stats {
            let avg_us = if s.count > 0 { s.elapsed_sum_us / s.count } else { 0 };
            let min_us = if s.elapsed_min_us == u64::MAX { 0 } else { s.elapsed_min_us };
            println!(
                "[actor-stats] app={} host={} tag={} count={} min_us={} avg_us={} max_us={} depth_max={}",
                app, host, s.tag, s.count, min_us, avg_us, s.elapsed_max_us, s.mailbox_depth_max,
            );
        }
    }));
}

fn report_metrics(
    background: &impl Fn(Runnable),
    app: &str,
    host: &str,
    scheduler: SchedulerMetrics,
    metrics: HashMap<String, Vec<(SystemTime, f64)>>,
) {
    let app = app.to_string();
    let host = host.to_string();

    background(Box::new(move || {
        println!("{:?}", scheduler);
        metrics
            .into_iter()
            .flat_map(|(tag, entries)| {
                entries
                    .into_iter()
                    .map(|(st, val)| MetricEntry {
                        at: st
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64,
                        meta: Meta {
                            host: host.clone(),
                            app: app.clone(),
                            tag: tag.clone(),
                        },
                        val,
                    })
                    .collect::<Vec<_>>()
            })
            .for_each(|e| println!("{:?}", e));
    }))
}
