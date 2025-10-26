# tos-perf-monitor

Performance monitoring library for TOS blockchain.

## Features

- System metrics collection (CPU, Memory, Disk I/O)
- Extensible TOS-specific metrics via `TosMetricsProvider` trait
- Low overhead (< 1% CPU, < 50MB memory)
- Background monitoring with configurable intervals
- Thread-safe snapshot-based architecture

## Usage

```rust
use std::sync::Arc;
use std::time::Duration;
use tos_perf_monitor::PerformanceMonitor;

#[tokio::main]
async fn main() {
    let monitor = Arc::new(PerformanceMonitor::new());

    // One-time snapshot
    let snapshot = monitor.snapshot().unwrap();
    println!("{}", snapshot);

    // Continuous monitoring
    let handle = monitor.clone().start_monitoring(
        Duration::from_secs(1),
        |snapshot| {
            println!("CPU: {:.1}%", snapshot.cpu_usage_percent);
        }
    );

    tokio::time::sleep(Duration::from_secs(10)).await;
    handle.stop().await;
}
```

## Examples

```bash
cargo run --example simple_monitor
```

## Documentation

See `/Users/tomisetsu/tos-network/memo/P3_MONITORING_DASHBOARD.md` for full documentation.
