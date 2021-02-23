use std::time::Duration;

#[derive(Default, Clone)]
pub struct Config {
    pub scheduler: SchedulerConfig,
    pub remote: RemoteConfig,
}

impl Config {
    pub fn new(scheduler: SchedulerConfig, remote: RemoteConfig) -> Config {
        Config {
            scheduler,
            remote,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub events_capacity: usize,
    pub recv_buffer_size: usize,
    pub send_buffer_size: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            events_capacity: 1024,
            recv_buffer_size: 1024,
            send_buffer_size: 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub events_capacity: usize,
    pub recv_buffer_size: usize,
    pub send_buffer_size: usize,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            events_capacity: 1024,
            recv_buffer_size: 1024,
            send_buffer_size: 1024,
        }
    }
}

#[derive(Clone, Default)]
pub struct RemoteConfig {
    pub enabled: bool,
    pub listening: String,
    pub server: ServerConfig,
    pub client: ClientConfig,
}

impl RemoteConfig {
    pub fn listening_at(listening: &str, server: ServerConfig, client: ClientConfig) -> Self {
        Self {
            enabled: true,
            listening: listening.to_string(),
            server,
            client,
        }
    }
}

#[derive(Clone)]
pub struct SchedulerConfig {
    // Maximum number of envelopes an actor can process at single scheduled execution
    pub actor_throughput: usize,
    pub default_mailbox_capacity: usize,
    pub logging_enabled: bool,
    pub metric_reporting_enabled: bool,
    pub metric_reporting_interval: Duration,
    pub delay_precision: Duration,
    pub actor_worker_threads: usize,
    pub extra_worker_threads: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        SchedulerConfig {
            actor_throughput: 1,
            default_mailbox_capacity: 16,
            logging_enabled: true,
            metric_reporting_enabled: false,
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
