use std::time::Duration;

#[derive(Default, Copy, Clone)]
pub struct Config {
    pub(crate) scheduler: SchedulerConfig,
}

impl Config {
    pub fn new(scheduler: SchedulerConfig) -> Config {
        Config {
            scheduler,
        }
    }
}

#[derive(Copy, Clone)]
pub struct SchedulerConfig {
    // Maximum number of envelopes an actor can process at single scheduled execution
    pub(crate) actor_throughput: usize,
    pub(crate) metric_reporting_interval: Duration,
    pub(crate) delay_precision: Duration,
    pub(crate) actor_worker_threads: usize,
    pub(crate) extra_worker_threads: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        SchedulerConfig {
            actor_throughput: 1,
            metric_reporting_interval: Duration::from_secs(1),
            delay_precision: Duration::from_millis(1),
            actor_worker_threads: 4,
            extra_worker_threads: 1,
        }
    }
}

impl SchedulerConfig {
    pub(crate) fn total_threads_required(&self) -> usize {
        self.actor_worker_threads + self.extra_worker_threads + 1
    }
}
