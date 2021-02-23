use std::fmt;

#[derive(Debug)]
pub struct LogEntry {
    pub at: u64,
    pub host: String,
    pub app: String,
    pub tag: String,
    pub log: String,
}

#[derive(Debug)]
pub struct MetricEntry {
    pub at: u64,
    pub host: String,
    pub app: String,
    pub tag: String,
    pub val: f64,
}

#[derive(Clone)]
pub struct SchedulerMetrics {
    pub name: String,
    pub at: u64,
    pub ticks: u64,
    pub miss: u64,
    pub hit: u64,
    pub messages: u64,
    pub queues: u64,
    pub returns: u64,
    pub spawns: u64,
    pub delays: u64,
    pub stops: u64,
    pub drops: u64,
    pub failures: u64,
    pub actors: u64,
}

impl SchedulerMetrics {
    pub fn named(name: String) -> Self {
        Self {
            name,
            at: 0,
            ticks: 0,
            miss: 0,
            hit: 0,
            messages: 0,
            queues: 0,
            returns: 0,
            spawns: 0,
            delays: 0,
            stops: 0,
            drops: 0,
            failures: 0,
            actors: 0,
        }
    }

    pub fn reset(&mut self) {
        self.at = 0;
        self.ticks = 0;
        self.miss = 0;
        self.hit = 0;
        self.messages = 0;
        self.queues = 0;
        self.returns = 0;
        self.spawns = 0;
        self.delays = 0;
        self.stops = 0;
        self.drops = 0;
        self.failures = 0;
        self.actors = 0;
    }
}

impl fmt::Debug for SchedulerMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchedulerMetrics")
            .field("name", &self.name)
            .field("at", &self.at)
            .field("ticks", &self.ticks)
            .field("miss", &self.miss)
            .field("hit", &self.hit)
            .field("messages", &self.messages)
            .field("queues", &self.queues)
            .field("returns", &self.returns)
            .field("spawns", &self.spawns)
            .field("delays", &self.delays)
            .field("stops", &self.stops)
            .field("drops", &self.drops)
            .field("failures", &self.failures)
            .field("actors", &self.actors)
            .finish()
    }
}
