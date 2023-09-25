# TOS

[![Build Status](https://github.com/tos-network/tos/actions/workflows/rust.yml/badge.svg)](https://github.com/tos-network/tos/actions/workflows/rust.yml)
[![License](https://img.shields.io/badge/license-Apache-green.svg)](LICENSE.md)

This repository is dedicated to sharing material related to the TOS Network.

## Summary

TOS allows a set of distributed authorities, some of which are Byzantine, to maintain a high-integrity and availability settlement system for pre-funded payments. It can be used to settle payments in a native unit of value (crypto-currency), or as a financial side-infrastructure to support retail payments in fiat currencies. TOS is based on Byzantine Consistent Broadcast as its core primitive, foregoing the expenses of full atomic commit channels (consensus). The resulting system has low-latency for both confirmation and payment finality. Remarkably, each validator can be sharded across many machines to allow unbounded horizontal scalability. Our experiments demonstrate intra-continental confirmation latency of less than 100ms, making TOS applicable to point of sale payments. In laboratory environments, we achieve over 80,000 transactions per second with 20 authorities---surpassing the requirements of current retail card payment networks, while significantly increasing their robustness.

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
