use std::fmt;

#[derive(Default, Copy, Clone)]
pub struct SchedulerMetrics {
    pub at: u64,
    pub millis: u64,
    pub ticks: u64,
    pub miss: u64,
    pub hit: u64,
    pub messages: u64,
    pub queues: u64,
    pub returns: u64,
    pub spawns: u64,
    pub delays: u64,
    pub stops: u64,
    pub actors: u64,
}

impl fmt::Debug for SchedulerMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchedulerMetrics")
            .field("at", &self.at)
            .field("millis", &self.millis)
            .field("ticks", &self.ticks)
            .field("miss", &self.miss)
            .field("hit", &self.hit)
            .field("messages", &self.messages)
            .field("queues", &self.queues)
            .field("returns", &self.returns)
            .field("spawns", &self.spawns)
            .field("delays", &self.delays)
            .field("stops", &self.stops)
            .field("actors", &self.actors)
            .finish()
    }
}
