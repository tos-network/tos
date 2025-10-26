# tos-stat-recorder

CSV statistics recorder for TOS performance metrics.

## Features

- Export metrics to CSV format
- Configurable sampling intervals
- Automatic directory creation
- Append mode for continuous recording
- Human-readable timestamps

## Usage

```rust
use std::sync::Arc;
use std::time::Duration;
use tos_perf_monitor::PerformanceMonitor;
use tos_stat_recorder::StatRecorder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let monitor = Arc::new(PerformanceMonitor::new());
    let recorder = StatRecorder::new("./metrics.csv", Duration::from_secs(1));

    let handle = recorder.start(monitor).await?;

    // Run your workload...
    tokio::time::sleep(Duration::from_secs(60)).await;

    handle.stop().await;
    println!("Metrics saved to: metrics.csv");
    Ok(())
}
```

## CSV Format

```csv
timestamp,uptime_secs,resident_set_size_mb,virtual_memory_size_mb,cpu_usage_percent,fd_count,disk_read_mb,disk_write_mb,disk_read_mbps,disk_write_mbps,mempool_size,confirmed_tps,pending_tps,avg_confirmation_time_ms,block_height
2025-10-26T07:30:00Z,120,245.32,512.48,45.2,128,1024.5,512.3,2.5,1.2,150,123.45,67.89,50.0,12345
```

## Examples

```bash
cargo run --example record_metrics
```

## Documentation

See `/Users/tomisetsu/tos-network/memo/P3_MONITORING_DASHBOARD.md` for full documentation.
