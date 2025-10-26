use std::sync::Arc;
use std::time::Duration;
use tos_perf_monitor::PerformanceMonitor;

#[tokio::main]
async fn main() {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    println!("Starting TOS Performance Monitor (simple example)");
    println!("Press Ctrl+C to stop\n");

    let monitor = Arc::new(PerformanceMonitor::new());

    // Start continuous monitoring
    let handle = monitor.clone().start_monitoring(Duration::from_secs(2), |snapshot| {
        println!("{}", snapshot);
    });

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");

    println!("\nStopping monitor...");
    handle.stop().await;
    println!("Monitor stopped");
}
