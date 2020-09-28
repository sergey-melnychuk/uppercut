#### Logging and Monitoring

##### Idea

Built-in log aggregation and metrics reporting - possibly overridden by exporters to other services (e.g. logstash, prometheus etc).

Simply reading stream of lines (separated by `\n`) from TCP and storing them to time series DB (in memory even) should be enough.

Persistence options: QuestDB (metrics), Cassandra (logging), flat files (logging).

Flat files might be super easy to start with for both metrics and logging.
Logging as plain text line by line to `/path/to/logs/host=192.168.42.42/app=server/topic=connections/tag=connection-00001/1599041920.log`.
Metrics flushed to binary file (fixed-length value: 8 bytes (double)): `/path/to/metrics/host=192.168.42.42/app=server/topic=connections/tag=connection-00001/1599041920.bin`.

Indexing/querying: ???

Admin API: increase logging level, enable/disable logging all messages (e.g. for specific Actor).

##### Misc

```rust
trait Logger {
    fn log<'a>(&mut self, tag: &str, message: &dyn Fn() -> String);
}

#[derive(Default)]
struct LocalLogger;

impl Logger for LocalLogger {
    fn log<'a>(&mut self, tag: &str, msg: &dyn Fn() -> String) {
        println!("2020-09-02 12:34:56.789 [host=127.0.0.1 app=server topic=connection tag={}] {}", tag, msg());
    }
}

fn main() {
    let mut logger = LocalLogger::default();
    let x = 42;
    logger.log("connection-00001", &|| format!("Client connection received from 192.168.1.123:56123, x={:?}", x));
}
```

A unified format for logs and metrics, prometheus-friendly:

```json
{"at":1599041920,"meta":{"host":"<host>","app":"server","topic":"events","tag":"connection-00001"},
    "log":"Client connection received from 192.168.1.123:56123, x=42"}
```

```json
{"at":1599041920,"meta":{"host":"<host>","app":"server","topic":"tick"},"val":669207.0}
{"at":1599041920,"meta":{"host":"<host>","app":"server","topic":"miss"},"val":0.0}
{"at":1599041920,"meta":{"host":"<host>","app":"server","topic":"hit"},"val":669207.0}
{"at":1599041920,"meta":{"host":"<host>","app":"server","topic":"messages"},"val":334433.0}
{"at":1599041920,"meta":{"host":"<host>","app":"server","topic":"queues"},"val":334433.0}
{"at":1599041920,"meta":{"host":"<host>","app":"server","topic":"returns"},"val":334691.0}
{"at":1599041920,"meta":{"host":"<host>","app":"server","topic":"spawns"},"val":0.0}
{"at":1599041920,"meta":{"host":"<host>","app":"server","topic":"delays"},"val":83.0}
{"at":1599041920,"meta":{"host":"<host>","app":"server","topic":"stops"},"val":0.0}
{"at":1599041920,"meta":{"host":"<host>","app":"server","topic":"actors"},"val":100004.0}
```

Lightweight approach to define local machines IP (if any):

```rust
// get_if_addrs = "0.5"

extern crate get_if_addrs;

use std::net::IpAddr;

fn main() {
    let ip = get_if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .find(|i| !i.is_loopback())
        .map(|i| i.addr.ip())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]));
    
    println!("IP: {}", ip.to_string());
}
```
