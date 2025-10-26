/// Example: Integration with TOS benchmarks
/// Shows how to add performance monitoring to existing benchmarks

use std::sync::Arc;
use std::time::Duration;
use tos_perf_monitor::PerformanceMonitor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== TOS Benchmark with Performance Monitoring ===\n");

    // Initialize monitor
    let monitor = Arc::new(PerformanceMonitor::new());

    // Start monitoring in background
    let monitor_clone = monitor.clone();
    let monitor_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if let Ok(snapshot) = monitor_clone.snapshot() {
                // Log key metrics during benchmark
                println!("[METRICS] CPU: {:.1}% | Memory: {:.0} MB | Disk R/W: {:.2}/{:.2} MB/s",
                    snapshot.cpu_usage_percent,
                    snapshot.resident_set_size as f64 / (1024.0 * 1024.0),
                    snapshot.disk_read_per_sec / (1024.0 * 1024.0),
                    snapshot.disk_write_per_sec / (1024.0 * 1024.0)
                );
            }
        }
    });

    // Simulate benchmark workload
    println!("Running benchmark for 10 seconds...\n");

    for i in 1..=10 {
        // Simulate some work
        tokio::time::sleep(Duration::from_secs(1)).await;
        println!("[BENCHMARK] Progress: {}0%", i);
    }

    println!("\nBenchmark complete!");
    println!("\n=== Final Metrics ===");

    let final_snapshot = monitor.snapshot()?;
    println!("{}", final_snapshot);

    // Stop monitoring
    monitor_handle.abort();

    println!("\n=== Usage in Real Benchmarks ===");
    println!("1. Add tos-perf-monitor dependency to daemon/Cargo.toml:");
    println!("   tos-perf-monitor = {{ path = \"../metrics/perf_monitor\" }}");
    println!();
    println!("2. In your benchmark (daemon/benches/tps.rs):");
    println!("   let monitor = Arc::new(PerformanceMonitor::new());");
    println!("   let recorder = StatRecorder::new(\"./tps_metrics.csv\", Duration::from_secs(1));");
    println!("   let handle = recorder.start(monitor).await?;");
    println!("   // Run benchmark...");
    println!("   handle.stop().await;");
    println!();
    println!("3. Analyze results:");
    println!("   python analyze_metrics.py tps_metrics.csv");

    Ok(())
}
