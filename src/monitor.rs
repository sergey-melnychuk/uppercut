#[derive(Debug)]
pub struct Meta {
    pub host: String,
    pub app: String,
    pub tag: String,
}

// TODO Add JSON support
#[derive(Debug)]
pub struct LoggerEntry {
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

#[derive(Default, Debug, Clone)]
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

#[cfg(feature = "actor-stats")]
#[derive(Debug, Clone)]
pub struct ActorStats {
    pub tag: String,
    pub count: u64,
    pub elapsed_sum_us: u64,
    pub elapsed_min_us: u64,
    pub elapsed_max_us: u64,
    pub mailbox_depth_max: usize,
}

#[cfg(feature = "actor-stats")]
impl ActorStats {
    pub fn new(tag: String) -> Self {
        Self {
            tag,
            count: 0,
            elapsed_sum_us: 0,
            elapsed_min_us: u64::MAX,
            elapsed_max_us: 0,
            mailbox_depth_max: 0,
        }
    }

    pub fn record_elapsed(&mut self, elapsed_us: u64) {
        self.count += 1;
        self.elapsed_sum_us += elapsed_us;
        if elapsed_us < self.elapsed_min_us {
            self.elapsed_min_us = elapsed_us;
        }
        if elapsed_us > self.elapsed_max_us {
            self.elapsed_max_us = elapsed_us;
        }
    }

    pub fn reset(&mut self) {
        self.count = 0;
        self.elapsed_sum_us = 0;
        self.elapsed_min_us = u64::MAX;
        self.elapsed_max_us = 0;
        self.mailbox_depth_max = 0;
    }
}

impl SchedulerMetrics {
    pub fn named(name: String) -> Self {
        Self {
            name,
            ..Default::default()
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
