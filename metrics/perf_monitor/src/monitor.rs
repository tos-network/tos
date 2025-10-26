use crate::{MetricsSnapshot, MonitorError};
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, CpuRefreshKind, MemoryRefreshKind};

/// Performance monitor that tracks system and application metrics
pub struct PerformanceMonitor {
    /// System information collector
    system: Arc<RwLock<System>>,

    /// Process ID being monitored
    pid: Pid,

    /// Start time of monitoring
    start_time: Instant,

    /// Last snapshot taken
    last_snapshot: Arc<RwLock<MetricsSnapshot>>,

    /// Last I/O measurements for rate calculation
    last_io_read: Arc<RwLock<u64>>,
    last_io_write: Arc<RwLock<u64>>,
    last_io_time: Arc<RwLock<Instant>>,

    /// TOS-specific metrics provider (optional)
    tos_metrics_provider: Option<Arc<dyn TosMetricsProvider>>,
}

/// Trait for providing TOS-specific metrics
/// This allows the daemon to inject blockchain-specific data
pub trait TosMetricsProvider: Send + Sync {
    /// Get current mempool size
    fn get_mempool_size(&self) -> usize;

    /// Get confirmed transactions per second
    fn get_confirmed_tps(&self) -> f64;

    /// Get pending transactions per second
    fn get_pending_tps(&self) -> f64;

    /// Get average confirmation time in milliseconds
    fn get_avg_confirmation_time_ms(&self) -> f64;

    /// Get current block height
    fn get_block_height(&self) -> u64;
}

impl PerformanceMonitor {
    /// Create a new performance monitor for the current process
    pub fn new() -> Self {
        let pid = Pid::from_u32(std::process::id());
        let system = System::new_with_specifics(
            RefreshKind::new()
                .with_processes(ProcessRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything())
                .with_cpu(CpuRefreshKind::everything()),
        );

        Self {
            system: Arc::new(RwLock::new(system)),
            pid,
            start_time: Instant::now(),
            last_snapshot: Arc::new(RwLock::new(MetricsSnapshot::default())),
            last_io_read: Arc::new(RwLock::new(0)),
            last_io_write: Arc::new(RwLock::new(0)),
            last_io_time: Arc::new(RwLock::new(Instant::now())),
            tos_metrics_provider: None,
        }
    }

    /// Set the TOS metrics provider
    pub fn with_tos_metrics(mut self, provider: Arc<dyn TosMetricsProvider>) -> Self {
        self.tos_metrics_provider = Some(provider);
        self
    }

    /// Get the process being monitored
    fn get_process(&self) -> Result<(), MonitorError> {
        let mut system = self.system.write();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            true,
            ProcessRefreshKind::everything()
        );

        if system.process(self.pid).is_some() {
            Ok(())
        } else {
            Err(MonitorError::ProcessNotFound)
        }
    }

    /// Take a snapshot of current metrics
    pub fn snapshot(&self) -> Result<MetricsSnapshot, MonitorError> {
        self.get_process()?;
        let now = Instant::now();

        let system = self.system.read();
        let process = system.process(self.pid).ok_or(MonitorError::ProcessNotFound)?;

        // System metrics
        let resident_set_size = process.memory();
        let virtual_memory_size = process.virtual_memory();

        // SAFE: f64 for display only, not consensus-critical
        let cpu_usage_percent = process.cpu_usage() as f64;

        // File descriptors (platform-specific)
        #[cfg(target_os = "linux")]
        let fd_count = {
            std::fs::read_dir(format!("/proc/{}/fd", self.pid.as_u32()))
                .map(|dir| dir.count() as u32)
                .unwrap_or(0)
        };
        #[cfg(not(target_os = "linux"))]
        let fd_count = 0;

        // Disk I/O
        let disk_info = process.disk_usage();
        let disk_read_bytes = disk_info.total_read_bytes;
        let disk_write_bytes = disk_info.total_written_bytes;

        // Calculate I/O rates
        let last_read = *self.last_io_read.read();
        let last_write = *self.last_io_write.read();
        let last_time = *self.last_io_time.read();
        let time_delta = now.duration_since(last_time).as_secs_f64();

        // SAFE: f64 for rate calculation display only
        let disk_read_per_sec = if time_delta > 0.0 {
            (disk_read_bytes.saturating_sub(last_read)) as f64 / time_delta
        } else {
            0.0
        };

        let disk_write_per_sec = if time_delta > 0.0 {
            (disk_write_bytes.saturating_sub(last_write)) as f64 / time_delta
        } else {
            0.0
        };

        // Update last I/O values
        *self.last_io_read.write() = disk_read_bytes;
        *self.last_io_write.write() = disk_write_bytes;
        *self.last_io_time.write() = now;

        // TOS-specific metrics
        let (mempool_size, confirmed_tps, pending_tps, avg_confirmation_time_ms, current_block_height) =
            if let Some(ref provider) = self.tos_metrics_provider {
                (
                    provider.get_mempool_size(),
                    provider.get_confirmed_tps(),
                    provider.get_pending_tps(),
                    provider.get_avg_confirmation_time_ms(),
                    provider.get_block_height(),
                )
            } else {
                (0, 0.0, 0.0, 0.0, 0)
            };

        let snapshot = MetricsSnapshot {
            timestamp: now,
            resident_set_size,
            virtual_memory_size,
            cpu_usage_percent,
            fd_count,
            disk_read_bytes,
            disk_write_bytes,
            disk_read_per_sec,
            disk_write_per_sec,
            mempool_size,
            confirmed_tps,
            pending_tps,
            avg_confirmation_time_ms,
            current_block_height,
        };

        // Store as last snapshot
        *self.last_snapshot.write() = snapshot;

        Ok(snapshot)
    }

    /// Get the last snapshot without refreshing
    pub fn last_snapshot(&self) -> MetricsSnapshot {
        *self.last_snapshot.read()
    }

    /// Get uptime duration
    pub fn uptime(&self) -> Duration {
        Instant::now().duration_since(self.start_time)
    }

    /// Start continuous monitoring in background
    /// Returns a handle that can be used to stop monitoring
    pub fn start_monitoring(
        self: Arc<Self>,
        interval: Duration,
        callback: impl Fn(MetricsSnapshot) + Send + 'static,
    ) -> MonitorHandle {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);

        let monitor = self.clone();
        let handle = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            interval_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = interval_timer.tick() => {
                        match monitor.snapshot() {
                            Ok(snapshot) => callback(snapshot),
                            Err(e) => {
                                if log::log_enabled!(log::Level::Error) {
                                    log::error!("Failed to collect metrics: {}", e);
                                }
                            }
                        }
                    }
                    _ = rx.recv() => {
                        if log::log_enabled!(log::Level::Info) {
                            log::info!("Stopping performance monitor");
                        }
                        break;
                    }
                }
            }
        });

        MonitorHandle { stop_tx: tx, handle }
    }
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle for controlling background monitoring
pub struct MonitorHandle {
    stop_tx: tokio::sync::mpsc::Sender<()>,
    handle: tokio::task::JoinHandle<()>,
}

impl MonitorHandle {
    /// Stop the background monitoring
    pub async fn stop(self) {
        let _ = self.stop_tx.send(()).await;
        let _ = self.handle.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestMetricsProvider;

    impl TosMetricsProvider for TestMetricsProvider {
        fn get_mempool_size(&self) -> usize {
            42
        }

        fn get_confirmed_tps(&self) -> f64 {
            123.45
        }

        fn get_pending_tps(&self) -> f64 {
            67.89
        }

        fn get_avg_confirmation_time_ms(&self) -> f64 {
            50.0
        }

        fn get_block_height(&self) -> u64 {
            12345
        }
    }

    #[test]
    fn test_monitor_creation() {
        let monitor = PerformanceMonitor::new();
        assert!(monitor.uptime().as_millis() < 100);
    }

    #[test]
    fn test_snapshot() {
        let monitor = PerformanceMonitor::new();
        let snapshot = monitor.snapshot();
        assert!(snapshot.is_ok());

        let snapshot = snapshot.unwrap();
        assert!(snapshot.resident_set_size > 0);
    }

    #[test]
    fn test_with_tos_metrics() {
        let provider = Arc::new(TestMetricsProvider);
        let monitor = PerformanceMonitor::new().with_tos_metrics(provider);

        let snapshot = monitor.snapshot().unwrap();
        assert_eq!(snapshot.mempool_size, 42);
        assert_eq!(snapshot.confirmed_tps, 123.45);
        assert_eq!(snapshot.current_block_height, 12345);
    }

    #[tokio::test]
    async fn test_continuous_monitoring() {
        let monitor = Arc::new(PerformanceMonitor::new());
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count_clone = count.clone();

        let handle = monitor.start_monitoring(Duration::from_millis(100), move |_snapshot| {
            count_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });

        tokio::time::sleep(Duration::from_millis(350)).await;
        handle.stop().await;

        let final_count = count.load(std::sync::atomic::Ordering::Relaxed);
        assert!(final_count >= 3 && final_count <= 4, "Expected 3-4 snapshots, got {}", final_count);
    }
}
