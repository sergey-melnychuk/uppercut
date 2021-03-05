#[derive(Debug)]
pub struct Meta {
    pub host: String,
    pub app: String,
    pub tag: String,
}

// TODO Add JSON support
#[derive(Debug)]
pub struct LogEntry {
    pub at: u64,
    pub meta: Meta,
    pub log: String,
}

// TODO Add JSON support
#[derive(Debug)]
pub struct MetricEntry {
    pub at: u64,
    pub meta: Meta,
    pub val: f64,
}

#[derive(Debug, Clone)]
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

