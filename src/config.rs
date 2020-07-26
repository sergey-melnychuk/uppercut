use std::time::Duration;

#[derive(Default, Copy, Clone)]
pub struct Config {
    pub scheduler: SchedulerConfig,
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
    pub actor_throughput: usize,
    pub metric_reporting_interval: Duration,
    pub delay_precision: Duration,
    pub actor_worker_threads: usize,
    pub extra_worker_threads: usize,
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
    pub fn with_total_threads(total_threads: usize) -> Self {
        let mut config = SchedulerConfig::default();
        config.actor_worker_threads = total_threads;
        config
    }

    pub(crate) fn total_threads_required(&self) -> usize {
        self.actor_worker_threads + self.extra_worker_threads + 1
    }
}
