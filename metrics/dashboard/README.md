# tos-metrics-dashboard

Real-time terminal UI dashboard for TOS performance monitoring.

## Features

- Real-time metrics display (1-second refresh)
- Visual gauges for CPU and TPS
- Sparkline charts (60-second history)
- Memory, disk I/O, and blockchain metrics
- Keyboard controls (`q` or `ESC` to quit)

## Usage

```bash
cargo run --bin tos-metrics-dashboard
```

Or build and run:

```bash
cargo build --release --bin tos-metrics-dashboard
./target/release/tos-metrics-dashboard
```

## Display

```
┌─ TOS Performance Monitor ─────────────────┐
│ Press 'q' or ESC to quit                   │
└────────────────────────────────────────────┘
┌─ CPU Usage ────────────────────────────────┐
│ ████████████████░░░░░░░░░░  45.2%         │
└────────────────────────────────────────────┘
┌─ Memory ───────────────────────────────────┐
│ RSS: 245 MB  |  Virtual: 512 MB  |  FDs: 128│
└────────────────────────────────────────────┘
```

## Requirements

- Terminal with UTF-8 support
- Minimum terminal size: 80x24 characters

## Documentation

See `/Users/tomisetsu/tos-network/memo/P3_MONITORING_DASHBOARD.md` for full documentation.
