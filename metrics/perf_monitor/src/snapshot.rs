use std::fmt::{self, Display};
use std::time::Instant;

/// Snapshot of system and application metrics at a specific point in time
#[derive(Debug, Clone, Copy)]
pub struct MetricsSnapshot {
    /// Timestamp when this snapshot was taken
    pub timestamp: Instant,

    // System resource metrics
    /// Resident Set Size - physical memory used (bytes)
    pub resident_set_size: u64,

    /// Virtual Memory Size - total virtual memory used (bytes)
    pub virtual_memory_size: u64,

    /// SAFE: f64 for display only, not consensus-critical
    /// CPU usage percentage (0.0-100.0 per core)
    pub cpu_usage_percent: f64,

    /// Number of open file descriptors
    pub fd_count: u32,

    // Disk I/O metrics
    /// Total bytes read from disk since process start
    pub disk_read_bytes: u64,

    /// Total bytes written to disk since process start
    pub disk_write_bytes: u64,

    /// SAFE: f64 for display only, not consensus-critical
    /// Current disk read rate (bytes/sec)
    pub disk_read_per_sec: f64,

    /// SAFE: f64 for display only, not consensus-critical
    /// Current disk write rate (bytes/sec)
    pub disk_write_per_sec: f64,

    // TOS-specific metrics
    /// Current mempool size (number of transactions)
    pub mempool_size: usize,

    /// SAFE: f64 for display only, not consensus-critical
    /// Confirmed transactions per second
    pub confirmed_tps: f64,

    /// SAFE: f64 for display only, not consensus-critical
    /// Pending transactions per second
    pub pending_tps: f64,

    /// SAFE: f64 for display only, not consensus-critical
    /// Average transaction confirmation time (milliseconds)
    pub avg_confirmation_time_ms: f64,

    /// Current blockchain height
    pub current_block_height: u64,
}

impl Default for MetricsSnapshot {
    fn default() -> Self {
        Self {
            timestamp: Instant::now(),
            resident_set_size: 0,
            virtual_memory_size: 0,
            cpu_usage_percent: 0.0,
            fd_count: 0,
            disk_read_bytes: 0,
            disk_write_bytes: 0,
            disk_read_per_sec: 0.0,
            disk_write_per_sec: 0.0,
            mempool_size: 0,
            confirmed_tps: 0.0,
            pending_tps: 0.0,
            avg_confirmation_time_ms: 0.0,
            current_block_height: 0,
        }
    }
}

impl MetricsSnapshot {
    /// Format bytes into human-readable string (e.g., "1.23 GB")
    /// SAFE: f64 for display formatting only
    fn format_bytes(bytes: u64) -> String {
        const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit_idx = 0;

        while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
            size /= 1024.0;
            unit_idx += 1;
        }

        if unit_idx == 0 {
            format!("{} {}", bytes, UNITS[0])
        } else {
            format!("{:.2} {}", size, UNITS[unit_idx])
        }
    }

    /// Format rate into human-readable string (e.g., "1.23 MB/s")
    /// SAFE: f64 for display formatting only
    fn format_rate(bytes_per_sec: f64) -> String {
        const UNITS: [&str; 5] = ["B/s", "KB/s", "MB/s", "GB/s", "TB/s"];
        let mut rate = bytes_per_sec;
        let mut unit_idx = 0;

        while rate >= 1024.0 && unit_idx < UNITS.len() - 1 {
            rate /= 1024.0;
            unit_idx += 1;
        }

        format!("{:.2} {}", rate, UNITS[unit_idx])
    }
}

impl Display for MetricsSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== TOS Performance Metrics ===")?;
        writeln!(f)?;
        writeln!(f, "System Resources:")?;
        writeln!(f, "  Memory RSS:    {}", Self::format_bytes(self.resident_set_size))?;
        writeln!(f, "  Memory Virtual: {}", Self::format_bytes(self.virtual_memory_size))?;
        writeln!(f, "  CPU Usage:     {:.2}%", self.cpu_usage_percent)?;
        writeln!(f, "  File Descriptors: {}", self.fd_count)?;
        writeln!(f)?;
        writeln!(f, "Disk I/O:")?;
        writeln!(f, "  Total Read:    {}", Self::format_bytes(self.disk_read_bytes))?;
        writeln!(f, "  Total Written: {}", Self::format_bytes(self.disk_write_bytes))?;
        writeln!(f, "  Read Rate:     {}", Self::format_rate(self.disk_read_per_sec))?;
        writeln!(f, "  Write Rate:    {}", Self::format_rate(self.disk_write_per_sec))?;
        writeln!(f)?;
        writeln!(f, "TOS Blockchain:")?;
        writeln!(f, "  Block Height:  {}", self.current_block_height)?;
        writeln!(f, "  Mempool Size:  {} txs", self.mempool_size)?;
        writeln!(f, "  Confirmed TPS: {:.2}", self.confirmed_tps)?;
        writeln!(f, "  Pending TPS:   {:.2}", self.pending_tps)?;
        writeln!(f, "  Avg Conf Time: {:.2} ms", self.avg_confirmation_time_ms)?;
        writeln!(f, "================================")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(MetricsSnapshot::format_bytes(512), "512 B");
        assert_eq!(MetricsSnapshot::format_bytes(1024), "1.00 KB");
        assert_eq!(MetricsSnapshot::format_bytes(1536), "1.50 KB");
        assert_eq!(MetricsSnapshot::format_bytes(1048576), "1.00 MB");
        assert_eq!(MetricsSnapshot::format_bytes(1073741824), "1.00 GB");
    }

    #[test]
    fn test_format_rate() {
        assert_eq!(MetricsSnapshot::format_rate(512.0), "512.00 B/s");
        assert_eq!(MetricsSnapshot::format_rate(1024.0), "1.00 KB/s");
        assert_eq!(MetricsSnapshot::format_rate(1536.0), "1.50 KB/s");
        assert_eq!(MetricsSnapshot::format_rate(1048576.0), "1.00 MB/s");
    }
}
