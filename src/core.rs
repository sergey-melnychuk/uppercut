use std::collections::{HashMap, BinaryHeap, HashSet};
use std::time::{Instant, Duration, SystemTime};
use std::cmp::Ordering;
use std::ops::Add;
use std::panic::{self, AssertUnwindSafe};

use crossbeam_channel::{unbounded, Sender, Receiver, SendError};

use crate::api::{Actor, AnyActor, AnySender, Envelope};
use crate::monitor::{LoggerEntry, SchedulerMetrics, MetricEntry, Meta};
use crate::config::{Config, SchedulerConfig};
use crate::pool::{ThreadPool, Runnable};
use crate::error::Error;
use crate::mailbox::Mailbox;

#[cfg(feature = "remote")]
use crate::remote::server::{Server, StartServer};
#[cfg(feature = "remote")]
use crate::remote::client::{Client, StartClient};

#[allow(dead_code)] // CLIENT const is unused when feature 'remote' is not enabled
const CLIENT: &str = "$CLIENT";

#[allow(dead_code)] // SERVER const is unused when feature 'remote' is not enabled
const SERVER: &str = "$SERVER";


impl AnySender for Local {
    fn me(&self) -> &str {
        &self.tag
    }

    fn myself(&self) -> String {
        self.tag.clone()
    }

    fn send(&mut self, address: &str, mut envelope: Envelope) {
        let tag = adjust_remote_address(address, &mut envelope).to_string();
        let action = Action::Queue { tag, queue: vec![envelope] };
        self.tx.send(action).unwrap();
    }

    fn spawn(&mut self, address: &str, f: fn() -> Actor) {
        let action = Action::Spawn { tag: address.to_string(), actor: f() };
        self.tx.send(action).unwrap();
    }

    fn delay(&mut self, address: &str, mut envelope: Envelope, duration: Duration) {
        let at = Instant::now().add(duration);
        let tag = adjust_remote_address(address, &mut envelope).to_string();
        let entry = Entry { at, tag, envelope };
        let action = Action::Delay { entry };
        self.tx.send(action).unwrap();
    }

    fn stop(&mut self, address: &str) {
        let action = Action::Stop { tag: address.to_string() };
        self.tx.send(action).unwrap();
    }

    fn log(&mut self, message: &str) {
        self.logs.push((self.now(), message.to_string()));
    }

    fn metric(&mut self, name: &str, value: f64) {
        let now = self.now();
        self.metrics.entry(name.to_string())
            .or_insert_with(Vec::default)
            .push((now, value));
    }

    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

impl Local {
    fn drain(&mut self, tx: &Sender<Action>) -> Result<(), SendError<Action>> {
        if !self.logs.is_empty() {
            let action = Action::Logs { tag: self.tag.clone(), logs: self.logs.to_owned() };
            tx.send(action)?;
            self.logs.clear();
        }
        if !self.metrics.is_empty() {
            let action = Action::Metrics { map: self.metrics.to_owned() };
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
    queue: HashMap<String, Mailbox>,
    tasks: BinaryHeap<Entry>,
    active: HashSet<String>,
}

impl Scheduler {
    fn with_config(config: &SchedulerConfig) -> Scheduler {
        Scheduler {
            config: config.clone(),
            actors: HashMap::default(),
            queue: HashMap::default(),
            tasks: BinaryHeap::default(),
            active: HashSet::default(),
        }
    }
}

// received by Worker threads
enum Event {
    Mail { tag: String, actor: Actor, queue: Vec<Envelope> },
    Stop { tag: String, actor: Actor },
    Shutdown,
}

// received by the Scheduler thread
enum Action {
    Return { tag: String, actor: Actor, ok: bool },
    Spawn { tag: String, actor: Actor },
    Queue { tag: String, queue: Vec<Envelope> },
    Delay { entry: Entry },
    Stop { tag: String },
    Logs { tag: String, logs: Vec<(SystemTime, String)> },
    Metrics { map: HashMap<String, Vec<(SystemTime, f64)>> },
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
        self.partial_cmp(other).unwrap()
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // Reverse ordering is required to turn max-heap (BinaryHeap) into min-heap.
        Some(self.at.cmp(&other.at).reverse())
    }
}

struct Runtime<'a> {
    name: String,
    host: String,
    pool: &'a ThreadPool,
    config: Config,
}

impl<'a> Runtime<'a> {
    fn new(name: String, host: String, pool: &'a ThreadPool, config: Config) -> Runtime {
        Runtime {
            name,
            host,
            pool,
            config,
        }
    }

    fn start(self) -> Result<Run<'a>, Error> {
        let (pool, config) = (self.pool, self.config);
        let events = unbounded();
        let actions = unbounded();
        let sender = actions.0.clone();

        start_actor_runtime(self.name, self.host, pool, config.scheduler, events, actions);
        let run = Run { sender, pool };

        #[cfg(feature = "remote")]
        if config.remote.enabled {
            let server = Server::listen(
                &config.remote.listening,
                &config.remote.server)?;
            let port = server.port();
            run.spawn(SERVER, move || Box::new(server));
            run.send(SERVER, Envelope::of(StartServer));
            let client = Client::new(port, &config.remote.client);
            run.spawn(CLIENT, move || Box::new(client));
            run.send(CLIENT, Envelope::of(StartClient));
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
            config: config.clone()
        }
    }

    pub fn run(self, pool: &ThreadPool) -> Result<Run, Error> {
        if pool.size() < self.config.scheduler.total_threads_required() {
            Err(Error::ThreadPoolTooSmall {
                required: self.config.scheduler.total_threads_required(),
                available: pool.size()
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
        let action = Action::Queue { tag: tag.to_string(), queue: vec![envelope] };
        self.sender.send(action).unwrap();
    }

    pub fn spawn<F: FnOnce() -> Actor>(&self, address: &str, f: F) {
        let action = Action::Spawn { tag: address.to_string(), actor: f() };
        self.sender.send(action).unwrap();
    }

    pub fn spawn_default<T: 'static + AnyActor + Send + Default>(&self, address: &str) {
        let action = Action::Spawn { tag: address.to_string(), actor: Box::new(T::default()) };
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
        let action = Action::Stop { tag: address.to_string() };
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

fn worker_loop(tx: Sender<Action>,
               rx: Receiver<Event>) {
    let mut sender = Local::new(tx.clone());
    loop {
        let event = rx.try_recv();
        if let Ok(e) = event {
            match e {
                Event::Mail { tag, mut actor, queue } => {
                    sender.tag = tag.clone();
                    let mut ok = true;
                    for envelope in queue.into_iter() {
                        let result = panic::catch_unwind(AssertUnwindSafe(|| {
                            actor.receive(envelope, &mut sender);
                        }));
                        if result.is_err() {
                            actor.on_fail(result.err().unwrap(), &mut sender);
                            ok = false;
                            break;
                        }
                    }
                    let sent = tx.send(Action::Return { tag, actor, ok });
                    if sent.is_err() {
                        break;
                    }
                },
                Event::Stop { tag, actor} => {
                    sender.tag = tag;
                    actor.on_stop(&mut sender);
                }
                Event::Shutdown => break
            }
            sender.drain(&tx).unwrap();
        }
    }
}

fn event_loop(actions_rx: Receiver<Action>,
              actions_tx: Sender<Action>,
              events_tx: Sender<Event>,
              mut scheduler: Scheduler,
              background: impl Fn(Runnable),
              name: String,
              host: String) {
    let mut scheduler_metrics = SchedulerMetrics::named(name.clone());
    let mut start = Instant::now();
    let mut logs = Vec::with_capacity(1024);
    let mut metrics = HashMap::with_capacity(1024);

    let min_timeout_millis: u64 = 1;
    let max_timeout_millis: u64 = 256;
    let mut timeout_millis = max_timeout_millis;
    'main: loop {
        let received = actions_rx.recv_timeout(Duration::from_millis(timeout_millis));
        if let Ok(action) = received {
            timeout_millis = std::cmp::max(min_timeout_millis, timeout_millis / 2);
            scheduler_metrics.hit += 1;
            match action {
                Action::Return { tag, actor, ok } if scheduler.active.contains(&tag) => {
                    if !ok {
                        scheduler_metrics.failures += 1;
                        scheduler.active.remove(&tag);
                        scheduler.queue.remove(&tag);
                        continue 'main;
                    }
                    scheduler_metrics.returns += 1;
                    let mailbox = scheduler.queue.get_mut(&tag).unwrap();
                    if mailbox.is_empty() {
                        scheduler.actors.insert(tag, actor);
                    } else {
                        let event = Event::Mail { tag, actor, queue: mailbox.get() };
                        events_tx.send(event).unwrap();
                    }
                },
                Action::Return { tag, actor, ok } => {
                    // Returned actor was stopped before (removed from active set).
                    scheduler.queue.remove(&tag);
                    if ok {
                        let event = Event::Stop { tag, actor };
                        events_tx.send(event).unwrap();
                    }
                },
                Action::Queue { tag, queue } if scheduler.active.contains(&tag) => {
                    scheduler_metrics.queues += 1;
                    scheduler_metrics.messages += queue.len() as u64;

                    let mailbox = scheduler.queue.get_mut(&tag).unwrap();
                    mailbox.put(queue);

                    if let Some(actor) = scheduler.actors.remove(&tag) {
                        let event = Event::Mail { tag, actor, queue: mailbox.get() };
                        events_tx.send(event).unwrap();
                    }
                },
                Action::Spawn { tag, actor } if !scheduler.active.contains(&tag) => {
                    scheduler_metrics.spawns += 1;
                    scheduler.active.insert(tag.clone());
                    scheduler.actors.insert(tag.clone(), actor);
                    let mailbox = Mailbox::new(
                        scheduler.config.actor_throughput,
                        scheduler.config.default_mailbox_capacity);
                    scheduler.queue.insert(tag.clone(), mailbox);
                },
                Action::Delay { entry } => {
                    scheduler_metrics.delays += 1 ;
                    scheduler.tasks.push(entry);
                },
                Action::Stop { tag } if scheduler.active.contains(&tag) => {
                    scheduler_metrics.stops += 1;
                    scheduler.active.remove(&tag);
                    scheduler.queue.remove(&tag);
                    if scheduler.actors.contains_key(&tag) {
                        let actor = scheduler.actors.remove(&tag).unwrap();
                        let event = Event::Stop { tag, actor };
                        events_tx.send(event).unwrap();
                    }
                },
                Action::Logs { tag, logs: entries } => {
                    logs.push((tag, entries));
                }
                Action::Metrics { map } => {
                    for (name, mut entries) in map {
                        metrics.entry(name)
                            .or_insert_with(Vec::default)
                            .append(&mut entries);
                    }
                },
                Action::Shutdown => break 'main,
                _ => {
                    scheduler_metrics.drops += 1;
                }
            }
        } else {
            timeout_millis = std::cmp::min(timeout_millis * 2, max_timeout_millis);
            scheduler_metrics.miss += 1;
        }

        let now = Instant::now().add(scheduler.config.delay_precision / 2);
        while scheduler.tasks.peek().map(|e| e.at <= now).unwrap_or_default() {
            if let Some(Entry { tag, envelope, .. }) = scheduler.tasks.pop() {
                let action = Action::Queue { tag, queue: vec![envelope] };
                actions_tx.send(action).unwrap();
            }
        }

        scheduler_metrics.ticks += 1;
        if start.elapsed() >= scheduler.config.metric_reporting_interval {
            let now = SystemTime::now();
            scheduler_metrics.at = now.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as u64;
            scheduler_metrics.actors = scheduler.active.len() as u64;

            if scheduler.config.metric_reporting_enabled {
                report_metrics(&background, &name, &host, scheduler_metrics.clone(), metrics);
                scheduler_metrics.reset();
                metrics = HashMap::with_capacity(1024);
            }

            if scheduler.config.logging_enabled {
                report_logs(&background, &name, &host, logs);
                logs = Vec::with_capacity(1024);
            }

            start = Instant::now();
        }

        if scheduler.config.eager_shutdown_enabled && scheduler.active.is_empty() {
            // Shutdown if there are not running actor (no progress can be made in such system).
            break 'main;
        }
    }
}

fn start_actor_runtime(name: String,
                       host: String,
                       pool: &ThreadPool,
                       scheduler_config: SchedulerConfig,
                       events: (Sender<Event>, Receiver<Event>),
                       actions: (Sender<Action>, Receiver<Action>)) {
    let (actions_tx, actions_rx) = actions;
    let (events_tx, events_rx) = events;

    let scheduler = Scheduler::with_config(&scheduler_config);

    let thread_count = scheduler.config.actor_worker_threads;
    for _ in 0..thread_count {
        let rx = events_rx.clone();
        let tx = actions_tx.clone();

        pool.submit(move || {
            worker_loop(tx, rx);
        });
    }

    let background = pool.link();
    pool.submit(move || {
        event_loop(actions_rx, actions_tx, events_tx.clone(), scheduler, background, name, host);
        for _ in 0..thread_count {
            events_tx.send(Event::Shutdown).unwrap();
        }
    });
}

fn adjust_remote_address<'a>(address: &'a str, _envelope: &'a mut Envelope) -> &'a str {
    #[cfg(feature = "remote")]
    if address.contains('@') {
        _envelope.to = address.to_string();
        return CLIENT;
    }
    address
}

fn report_logs(background: &impl Fn(Runnable),
               app: &str,
               host: &str,
               logs: Vec<(String, Vec<(SystemTime, String)>)>) {
    let app = app.to_string();
    let host = host.to_string();
    background(Box::new(move || {
        for (tag, stm) in logs {
            for (st, msg) in stm {
                let at = st.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as u64;
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

fn report_metrics(background: &impl Fn(Runnable),
                  app: &str,
                  host: &str,
                  scheduler: SchedulerMetrics,
                  metrics: HashMap<String, Vec<(SystemTime, f64)>>) {
    let app = app.to_string();
    let host = host.to_string();

    background(Box::new(move || {
        println!("{:?}", scheduler);
        let entries: Vec<MetricEntry> = metrics.into_iter()
            .flat_map(|(tag, entries)| {
                entries.into_iter()
                    .map(|(st, val)| {
                        MetricEntry {
                            at: st.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as u64,
                            meta: Meta {
                                host: host.clone(),
                                app: app.clone(),
                                tag: tag.clone(),
                            },
                            val,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        entries.into_iter().for_each(|e| println!("{:?}", e));
    }))
}
