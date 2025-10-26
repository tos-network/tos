use chrono::Utc;
use csv::Writer;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tos_perf_monitor::{MetricsSnapshot, PerformanceMonitor};

#[derive(Error, Debug)]
pub enum RecorderError {
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Monitor error: {0}")]
    Monitor(#[from] tos_perf_monitor::MonitorError),
}

/// Statistics recorder that exports metrics to CSV
pub struct StatRecorder {
    output_path: PathBuf,
    interval: Duration,
}

/// CSV record format for metrics
#[derive(Debug, serde::Serialize)]
struct MetricsRecord {
    timestamp: String,
    uptime_secs: u64,
    resident_set_size_mb: f64,
    virtual_memory_size_mb: f64,
    cpu_usage_percent: f64,
    fd_count: u32,
    disk_read_mb: f64,
    disk_write_mb: f64,
    disk_read_mbps: f64,
    disk_write_mbps: f64,
    mempool_size: usize,
    confirmed_tps: f64,
    pending_tps: f64,
    avg_confirmation_time_ms: f64,
    block_height: u64,
}

impl From<(MetricsSnapshot, u64)> for MetricsRecord {
    fn from((snapshot, uptime_secs): (MetricsSnapshot, u64)) -> Self {
        const MB: f64 = 1024.0 * 1024.0;

        Self {
            timestamp: Utc::now().to_rfc3339(),
            uptime_secs,
            // SAFE: f64 for CSV export display only
            resident_set_size_mb: snapshot.resident_set_size as f64 / MB,
            virtual_memory_size_mb: snapshot.virtual_memory_size as f64 / MB,
            cpu_usage_percent: snapshot.cpu_usage_percent,
            fd_count: snapshot.fd_count,
            disk_read_mb: snapshot.disk_read_bytes as f64 / MB,
            disk_write_mb: snapshot.disk_write_bytes as f64 / MB,
            disk_read_mbps: snapshot.disk_read_per_sec / MB,
            disk_write_mbps: snapshot.disk_write_per_sec / MB,
            mempool_size: snapshot.mempool_size,
            confirmed_tps: snapshot.confirmed_tps,
            pending_tps: snapshot.pending_tps,
            avg_confirmation_time_ms: snapshot.avg_confirmation_time_ms,
            block_height: snapshot.current_block_height,
        }
    }
}

impl StatRecorder {
    /// Create a new stat recorder
    pub fn new(output_path: impl Into<PathBuf>, interval: Duration) -> Self {
        Self {
            output_path: output_path.into(),
            interval,
        }
    }

    /// Start recording metrics to CSV
    /// Returns a handle to stop recording
    pub async fn start(
        &self,
        monitor: Arc<PerformanceMonitor>,
    ) -> Result<RecorderHandle, RecorderError> {
        let output_path = self.output_path.clone();
        let interval = self.interval;

        // Create output directory if it doesn't exist
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Initialize CSV writer with header
        let file = std::fs::File::create(&output_path)?;
        let mut writer = Writer::from_writer(file);

        // Write header
        writer.write_record(&[
            "timestamp",
            "uptime_secs",
            "resident_set_size_mb",
            "virtual_memory_size_mb",
            "cpu_usage_percent",
            "fd_count",
            "disk_read_mb",
            "disk_write_mb",
            "disk_read_mbps",
            "disk_write_mbps",
            "mempool_size",
            "confirmed_tps",
            "pending_tps",
            "avg_confirmation_time_ms",
            "block_height",
        ])?;
        writer.flush()?;

        // Drop the writer so we can reopen in append mode
        drop(writer);

        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);

        let handle = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            interval_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = interval_timer.tick() => {
                        match Self::record_snapshot(&output_path, &monitor).await {
                            Ok(_) => {},
                            Err(e) => {
                                if log::log_enabled!(log::Level::Error) {
                                    log::error!("Failed to record metrics: {}", e);
                                }
                            }
                        }
                    }
                    _ = rx.recv() => {
                        if log::log_enabled!(log::Level::Info) {
                            log::info!("Stopping stat recorder");
                        }
                        break;
                    }
                }
            }
        });

        Ok(RecorderHandle { stop_tx: tx, handle })
    }

    async fn record_snapshot(
        output_path: &PathBuf,
        monitor: &PerformanceMonitor,
    ) -> Result<(), RecorderError> {
        let snapshot = monitor.snapshot()?;
        let uptime_secs = monitor.uptime().as_secs();

        let record: MetricsRecord = (snapshot, uptime_secs).into();

        // Open in append mode
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(output_path)?;

        let mut writer = Writer::from_writer(file);
        writer.serialize(record)?;
        writer.flush()?;

        Ok(())
    }
}

/// Handle for controlling background recording
pub struct RecorderHandle {
    stop_tx: tokio::sync::mpsc::Sender<()>,
    handle: tokio::task::JoinHandle<()>,
}

impl RecorderHandle {
    /// Stop the background recording
    pub async fn stop(self) {
        let _ = self.stop_tx.send(()).await;
        let _ = self.handle.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_stat_recorder() {
        let temp_dir = tempdir().unwrap();
        let output_path = temp_dir.path().join("metrics.csv");

        let monitor = Arc::new(PerformanceMonitor::new());
        let recorder = StatRecorder::new(&output_path, Duration::from_millis(100));

        let handle = recorder.start(monitor).await.unwrap();

        tokio::time::sleep(Duration::from_millis(350)).await;
        handle.stop().await;

        // Verify CSV file was created and has content
        let content = tokio::fs::read_to_string(&output_path).await.unwrap();
        assert!(content.contains("timestamp"));
        assert!(content.contains("resident_set_size_mb"));

        // Count lines (header + data rows)
        let lines: Vec<_> = content.lines().collect();
        assert!(lines.len() >= 4, "Expected at least 4 lines (header + 3 records), got {}", lines.len());
    }
}
