# TOS Network

[![Build Status](https://github.com/tos-network/tos/actions/workflows/rust.yml/badge.svg)](https://github.com/tos-network/tos/actions/workflows/rust.yml)
[![License](https://img.shields.io/badge/license-Apache-green.svg)](LICENSE.md)

TOS Network is a genuine Web3.0 network with decentralized storage, instant payments and various decentralized services.

## Summary

TOS allows a set of distributed validators, some of which are Byzantine, to maintain a high-integrity and availability settlement system for fast payments. TOS has low-latency for both confirmation and payment finality. Remarkably, each validator can be sharded across many machines to allow unbounded horizontal scalability.

We achieve over 80,000 transactions per second with 20 validators---surpassing the requirements of current VISA card payment networks. TOS Network is very suitable to be used in AI, IoT services.

## Quickstart with TOS Network

```bash
cargo build --release
scripts/test.sh

# Benchmark
cd target/release
./bench

cd ../..
```


## License

The content of this repository is licensed as [Apache 2.0](https://github.com/tos-network/tos/blob/master/LICENSE)
