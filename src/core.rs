use std::error::Error;
use std::any::Any;
use std::collections::{HashMap, BinaryHeap, HashSet};
use std::sync::{Mutex, Arc};
use std::time::{Instant, Duration, SystemTime};
use std::cmp::Ordering;
use std::ops::Add;

use crossbeam_channel::{unbounded, Sender, Receiver};

use crate::api::{Actor, AnyActor, AnySender, Envelope};
use crate::metrics::{SchedulerMetrics};
use crate::config::{Config, SchedulerConfig};
use crate::pool::{ThreadPool, Runnable};
use crate::remote::server::{Server, StartServer};
use crate::remote::client::{Client, StartClient};

impl AnySender for Memory<Envelope> {
    fn me(&self) -> &str {
        &self.own
    }

    fn myself(&self) -> String {
        self.own.clone()
    }

    fn send(&mut self, address: &str, message: Envelope) {
        self.map.entry(address.to_string()).or_default().push(message);
    }

    fn spawn(&mut self, address: &str, f: fn() -> Actor) {
        self.new.insert(address.to_string(), f());
    }

    fn delay(&mut self, address: &str, envelope: Envelope, duration: Duration) {
        let at = Instant::now().add(duration);
        let entry = Entry { at, tag: address.to_string(), envelope };
        self.delay.push(entry);
    }

    fn stop(&mut self, address: &str) {
        self.stop.insert(address.to_string());
    }
}

impl Memory<Envelope> {
    fn drain(&mut self, tx: &Sender<Action>) {
        for (tag, actor) in self.new.drain().into_iter() {
            let action = Action::Spawn { tag, actor };
            tx.send(action).unwrap();
        }
        for (tag, queue) in self.map.drain().into_iter() {
            let action = Action::Queue { tag, queue };
            tx.send(action).unwrap();
        }
        for entry in self.delay.drain(..).into_iter() {
            let action = Action::Delay { entry };
            tx.send(action).unwrap();
        }
        for tag in self.stop.drain().into_iter() {
            let action = Action::Stop { tag };
            tx.send(action).unwrap_or_default();
        }
    }
}

#[derive(Default)]
struct Memory<T: Any + Sized + Send> {
    own: String,
    map: HashMap<String, Vec<T>>,
    new: HashMap<String, Actor>,
    delay: Vec<Entry>,
    stop: HashSet<String>,
}

struct Scheduler {
    config: SchedulerConfig,
    actors: HashMap<String, Actor>,
    queue: HashMap<String, Vec<Envelope>>,
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
    Stop,
}

// received by the Scheduler thread
enum Action {
    Return { tag: String, actor: Actor },
    Spawn { tag: String, actor: Actor },
    Queue { tag: String, queue: Vec<Envelope> },
    Delay { entry: Entry },
    Stop { tag: String },
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
        self.at.cmp(&other.at)
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.at.cmp(&other.at))
    }
}

struct Runtime<'a> {
    pool: &'a ThreadPool,
    config: Config,
}

impl<'a> Runtime<'a> {
    fn new(pool: &'a ThreadPool, config: Config) -> Runtime {
        Runtime {
            pool,
            config,
        }
    }

    fn start(self) -> Result<Run<'a>, Box<dyn Error>> {
        let (pool, config) = (self.pool, self.config);
        let (scheduler, remote) = (config.scheduler, config.remote);
        let events = unbounded();
        let actions = unbounded();
        let sender = actions.0.clone();

        start_actor_runtime(pool, scheduler, events, actions);
        let run = Run { sender, pool };

        if remote.enabled {
            let server = Server::listen(&remote.listening)?;
            let port = server.port();
            run.spawn("server", move || Box::new(server));
            run.send("server", Envelope::of(StartServer));

            let client = Client::new(port);
            run.spawn("client", move || Box::new(client));
            run.send("client", Envelope::of(StartClient));
        }

        Ok(run)
    }
}

#[derive(Default)]
pub struct System {
    config: Config,
}

impl System {
    pub fn new(config: &Config) -> System {
        System {
            config: config.clone()
        }
    }

    pub fn run(self, pool: &ThreadPool) -> Result<Run, Box<dyn Error>> {
        if pool.size() < self.config.scheduler.total_threads_required() {
            let e: Box<dyn Error> = "Not enough threads in the pool".to_string().into();
            Err(e)
        } else {
            let runtime = Runtime::new(pool, self.config);
            Ok(runtime.start()?)
        }
    }
}

pub struct Run<'a> {
    pool: &'a ThreadPool,
    sender: Sender<Action>,
}

impl<'a> Run<'a> {
    pub fn send(&self, address: &str, message: Envelope) {
        let action = Action::Queue { tag: address.to_string(), queue: vec![message] };
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

    pub fn delay(&self, address: &str, envelope: Envelope, duration: Duration) {
        let at = Instant::now().add(duration);
        let entry = Entry { at, tag: address.to_string(), envelope };
        let action = Action::Delay { entry };
        self.sender.send(action).unwrap();
    }

    pub fn stop(&self, address: &str) {
        let action = Action::Stop { tag: address.to_string() };
        self.sender.send(action).unwrap();
    }

    pub fn shutdown(self) {
        let action = Action::Shutdown;
        self.sender.send(action).unwrap();
    }

    pub fn submit<F: FnOnce() + Send + 'static>(&self, f: F) {
        self.pool.submit(f);
    }
}

fn worker_loop(tx: Sender<Action>,
               rx: Arc<Mutex<Receiver<Event>>>) {
    let mut memory: Memory<Envelope> = Memory::default();
    loop {
        let event = rx.lock().unwrap().try_recv();
        if let Ok(x) = event {
            match x {
                Event::Mail { tag, mut actor, queue } => {
                    memory.own = tag.clone();
                    for envelope in queue.into_iter() {
                        actor.receive(envelope, &mut memory);
                    }
                    let sent = tx.send(Action::Return { tag, actor });
                    if sent.is_err() {
                        break;
                    }
                },
                Event::Stop => break
            }
            memory.drain(&tx);
        }
    }
}

fn event_loop(actions_rx: Receiver<Action>,
              actions_tx: Sender<Action>,
              events_tx: Sender<Event>,
              mut scheduler: Scheduler,
              pool_link: impl Fn(Runnable)) {
    let throughput = scheduler.config.actor_throughput;
    let mut metrics = SchedulerMetrics::default();
    let mut start = Instant::now();
    loop {
        let received = actions_rx.try_recv();
        if let Ok(action) = received {
            metrics.hit += 1;
            match action {
                Action::Return { tag, actor } => {
                    if scheduler.active.contains(&tag) {
                        metrics.returns += 1;
                        let mut queue = scheduler.queue.remove(&tag).unwrap_or_default();
                        if queue.is_empty() {
                            scheduler.actors.insert(tag, actor);
                        } else {
                            if queue.len() > throughput {
                                let remaining = queue.split_off(throughput);
                                scheduler.queue.insert(tag.clone(), remaining);
                            }
                            let event = Event::Mail { tag, actor, queue };
                            events_tx.send(event).unwrap();
                        }
                    }
                },
                Action::Queue { tag, queue: added } => {
                    if scheduler.active.contains(&tag) {
                        metrics.queues += 1;
                        metrics.messages += added.len() as u64;

                        let mut queue = scheduler.queue.remove(&tag).unwrap_or_default();
                        queue.extend(added.into_iter());

                        if let Some(actor) = scheduler.actors.remove(&tag) {
                            if queue.len() > throughput {
                                let remaining = queue.split_off(throughput);
                                scheduler.queue.insert(tag.clone(), remaining);
                            }

                            let event = Event::Mail { tag: tag.clone(), actor, queue };
                            events_tx.send(event).unwrap();
                        } else {
                            scheduler.queue.insert(tag, queue);
                        }
                    }
                },
                Action::Spawn { tag, actor } => {
                    if !scheduler.active.contains(&tag) {
                        metrics.spawns += 1;
                        scheduler.active.insert(tag.clone());
                        scheduler.actors.insert(tag.clone(), actor);
                        scheduler.queue.insert(tag.clone(), Vec::with_capacity(32));
                    }
                },
                Action::Delay { entry } => {
                    metrics.delays +=1 ;
                    scheduler.tasks.push(entry);
                },
                Action::Stop { tag } => {
                    metrics.stops += 1;
                    scheduler.active.remove(&tag);
                    scheduler.actors.remove(&tag);
                    scheduler.queue.remove(&tag);
                },
                Action::Shutdown => break,
            }
        } else {
            metrics.miss +=1 ;
        }

        let now = Instant::now().add(scheduler.config.delay_precision);
        while scheduler.tasks.peek().map(|e| e.at <= now).unwrap_or_default() {
            if let Some(Entry { tag, envelope, .. }) = scheduler.tasks.pop() {
                let action = Action::Queue { tag, queue: vec![envelope] };
                actions_tx.send(action).unwrap();
            }
        }

        metrics.ticks += 1;
        if start.elapsed() >= scheduler.config.metric_reporting_interval {
            let now = SystemTime::now();
            metrics.at = now.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
            metrics.millis = start.elapsed().as_millis() as u64;
            metrics.actors = scheduler.active.len() as u64;

            // Feature required: configurable metric reporting
            if scheduler.config.metric_reporting_enabled {
                pool_link(Box::new(move || println!("{:?}", metrics.clone())));
            }

            metrics = SchedulerMetrics::default();
            start = Instant::now();
        }
    }
}

fn start_actor_runtime(pool: &ThreadPool,
                       scheduler_config: SchedulerConfig,
                       events: (Sender<Event>, Receiver<Event>),
                       actions: (Sender<Action>, Receiver<Action>)) {
    let (actions_tx, actions_rx) = actions;
    let (events_tx, events_rx) = events;
    let events_rx = Arc::new(Mutex::new(events_rx));

    let scheduler = Scheduler::with_config(&scheduler_config);

    let thread_count = scheduler.config.actor_worker_threads;
    for _ in 0..thread_count {
        let rx = Arc::clone(&events_rx);
        let tx = actions_tx.clone();

        pool.submit(move || {
            worker_loop(tx, rx);
        });
    }

    let pool_link = pool.link();
    pool.submit(move || {
        event_loop(actions_rx, actions_tx, events_tx.clone(), scheduler, pool_link);
        for _ in 0..thread_count {
            events_tx.send(Event::Stop).unwrap();
        }
    });
}
