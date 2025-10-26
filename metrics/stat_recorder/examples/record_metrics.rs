use std::sync::Arc;
use std::time::Duration;
use tos_perf_monitor::PerformanceMonitor;
use tos_stat_recorder::StatRecorder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    println!("Starting TOS Performance Monitor with CSV recording");
    println!("Recording to: ./bench_metrics.csv");
    println!("Press Ctrl+C to stop\n");

    let monitor = Arc::new(PerformanceMonitor::new());

    // Start CSV recording
    let recorder = StatRecorder::new("./bench_metrics.csv", Duration::from_secs(1));
    let recorder_handle = recorder.start(monitor.clone()).await?;

    // Also print to console
    let monitor_handle = monitor.clone().start_monitoring(Duration::from_secs(5), |snapshot| {
        println!("CPU: {:.1}% | Memory: {:.2} MB | TPS: {:.2}",
            snapshot.cpu_usage_percent,
            snapshot.resident_set_size as f64 / (1024.0 * 1024.0),
            snapshot.confirmed_tps
        );
    });

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;

    println!("\nStopping...");
    recorder_handle.stop().await;
    monitor_handle.stop().await;
    println!("Metrics saved to bench_metrics.csv");

    Ok(())
}
