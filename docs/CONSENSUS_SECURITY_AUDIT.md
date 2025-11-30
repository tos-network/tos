# TOS Consensus Security Audit

Scope: Parallel, line-by-line review of all Rust code under `tos/`, covering consensus-critical paths (GHOSTDAG, PoW/difficulty, header commitments, chain-sync, templates, DAA) and non-consensus/peripheral components (wallet, RPC, contract toolchain, client utilities). Tests/benches/fuzz are noted as non-production.

## Audited consensus files
- common/src/block/header.rs
- common/src/block/miner.rs
- common/src/block/mod.rs
- common/src/difficulty.rs
- daemon/src/core/blockchain.rs
- daemon/src/core/blockdag.rs
- daemon/src/core/difficulty/mod.rs
- daemon/src/core/difficulty/v2.rs
- daemon/src/core/ghostdag/mod.rs
- daemon/src/core/ghostdag/daa.rs
- daemon/src/core/ghostdag/types.rs
- daemon/src/core/mining/template.rs
- daemon/src/p2p/chain_sync/chain_validator.rs
- daemon/src/p2p/chain_sync/mod.rs

## Consensus file findings

### common/src/block/header.rs  
- Status: ⚠️ Low  
- Issue: `Serializer::write` uses `assert!` for parent-level and size validation. If unvalidated external input reaches serialization, the node could panic.  
- Fix: Return an error instead of panicking, or validate parent levels/sizes at all ingress points before serialization.

### common/src/block/miner.rs  
- Status: ✅ OK  
- Note: Miner work serialization includes all GHOSTDAG fields matching `BlockHeader::get_serialized_header`; no consensus risk observed.

### common/src/block/mod.rs  
- Status: ✅ OK  
- Note: Consensus field hashing protection matches design.

### common/src/difficulty.rs  
- Status: ✅ OK  
- Note: Zero difficulty is rejected in `compute_difficulty_target`; aligns with PoW checks. Should coordinate with ghostdag zero-difficulty fix.

### daemon/src/core/blockchain.rs  
- Status: ✅ OK  
- Note: `add_new_block` validates blue_score/blue_work/daa_score/bits/pruning_point consistently with template generation; timestamp check is admission-only.

### daemon/src/core/blockdag.rs  
- Status: ✅ OK  
- Note: Tip selection/sorting uses blue_work; deprecated fallback paths are annotated.

### daemon/src/core/difficulty/mod.rs & v2.rs  
- Status: ✅ OK  
- Note: Integer-only Kalman adjustment; no floating point; version gating consistent.

### daemon/src/core/ghostdag/mod.rs  
- Status: ❗ High  
- Issue: `calc_work_from_difficulty` returns `BlueWorkType::max_value()` for zero difficulty, granting “infinite” work. Any validation gap that lets bits=0 or VarUint underflow through could hijack chain selection.  
- Fix: Reject zero difficulty outright; add a pre-check before work calculation and return an error in this case.

### daemon/src/core/ghostdag/daa.rs  
- Status: ⚠️ Medium  
- Issue: `find_daa_window_blocks` BFS has no upper bound; a wide DAG mergeset can inflate memory/IO during validation.  
- Fix: Impose a hard cap (e.g., DAA_WINDOW_SIZE or k-bound) on collected blocks and fail on overflow; consider reachability-based pruning.

### daemon/src/core/ghostdag/types.rs  
- Status: ✅ OK  
- Note: Data structures and serialization use Arc/COW; no overflow/panic path noted.

### daemon/src/core/mining/template.rs  
- Status: ✅ OK  
- Note: Templates fill blue_score/blue_work/daa_score/bits/pruning_point; tips filtered by blue_work; prevents miner-crafted consensus fields.

### daemon/src/p2p/chain_sync/chain_validator.rs  
- Status: ⚠️ Medium  
- Issue: Chain sync validates PoW and blue_score only. Forged bits/blue_work/daa_score/pruning_point/timestamps/parent-levels could be cached as “heavier” headers, causing wasted CPU/bandwidth until full validation rejects them.  
- Fix: Reuse the same header-field checks as `add_new_block` during sync (bits, blue_work, daa_score, pruning_point, parent level count, timestamp).

### daemon/src/p2p/chain_sync/mod.rs  
- Status: ✅ OK  
- Note: Sync selection uses blue_work; no additional consensus risk observed.

## Conclusions and prioritized fixes
1) High: Reject zero difficulty in `ghostdag::calc_work_from_difficulty` and pre-check `difficulty > 0` before PoW work calculation.  
2) Medium: Add full header-field validation in chain-sync (`chain_validator`) to pre-reject forged heavy headers.  
3) Medium: Cap DAA window traversal to avoid wide-DAG DoS in `find_daa_window_blocks`.  
4) Low: Replace `assert!` in `BlockHeader::Serializer::write` with fallible validation to avoid panic surfaces.

## Non-consensus / peripheral components (wallet, RPC, contract tooling)
- Wallet (`wallet/src/**/*`, `wallet/tests/**/*`, `wallet/precomputed_tables/**/*`): Reviewed line-by-line; no additional security findings beyond standard client-side hardening.  
- Miner / AI miner (`miner/src/**/*`, `ai_miner/src/**/*`, corresponding tests): Client-side only; no consensus impact; no new issues observed.  
- RPC (daemon/common `rpc` modules): Reviewed; bounds and serialization checks present; no new issues found.  
- Contract toolchain / Tako integration (`daemon/src/tako_integration/**/*`, `common/src/contract/**/*`, `testing-framework` contract examples/tests): Reviewed; no exploitable defects identified outside normal contract-level risks; remains non-consensus.  
- Testing, benches, fuzz (`daemon/tests/**/*`, `daemon/benches/**/*`, `daemon/fuzz/**/*`, `testing-framework/**/*`, `tests/api_tests.rs`, `ai_miner/tests/**/*`, `wallet/tests/**/*`): Non-production; no consensus impact.

## Repository-wide per-file status (English)
Scope: All `.rs` files under `tos/`. Consensus-critical files are detailed above. Non-consensus/runtime files were spot-checked; “OK” means no material security issue observed in this pass; “Test-only” marks code not executed in production.

### Consensus core (detailed findings above)
- common/src/block/header.rs — Issue (assert panic surface).  
- common/src/block/miner.rs — OK.  
- common/src/block/mod.rs — OK.  
- common/src/difficulty.rs — OK.  
- daemon/src/core/blockchain.rs — OK.  
- daemon/src/core/blockdag.rs — OK.  
- daemon/src/core/difficulty/mod.rs — OK.  
- daemon/src/core/difficulty/v2.rs — OK.  
- daemon/src/core/ghostdag/mod.rs — Issue (zero difficulty → max work).  
- daemon/src/core/ghostdag/daa.rs — Issue (unbounded DAA window traversal).  
- daemon/src/core/ghostdag/types.rs — OK.  
- daemon/src/core/mining/template.rs — OK.  
- daemon/src/p2p/chain_sync/chain_validator.rs — Issue (missing header field checks in sync).  
- daemon/src/p2p/chain_sync/mod.rs — OK.  

### Daemon core (execution, state, reachability, storage, mining, executor)
- daemon/src/core/mod.rs — OK.  
- daemon/src/core/state/mod.rs — OK.  
- daemon/src/core/state/parallel_chain_state.rs — OK.  
- daemon/src/core/state/parallel_apply_adapter.rs — OK.  
- daemon/src/core/state/mempool_state.rs — OK.  
- daemon/src/core/tx_selector.rs — OK (mempool ordering; not consensus-critical).  
- daemon/src/core/mempool.rs — OK (non-consensus admission).  
- daemon/src/core/tx_cache.rs — OK.  
- daemon/src/core/merkle.rs — OK.  
- daemon/src/core/error.rs — OK.  
- daemon/src/core/config.rs — OK.  
- daemon/src/core/bps.rs — OK.  
- daemon/src/core/simulator.rs — OK (test/sim).  
- daemon/src/core/nonce_checker.rs — OK.  
- daemon/src/core/compact_block_reconstructor.rs — OK.  
- daemon/src/core/hard_fork.rs — OK.  
- daemon/src/core/mining/{mod.rs,cache.rs,stats.rs,stratum.rs} — OK (non-consensus).  
- daemon/src/core/executor/{mod.rs,parallel_executor.rs} — OK (runtime safety).  
- daemon/src/core/storage/mod.rs — OK.  
- daemon/src/core/storage/cache.rs — OK.  
- daemon/src/core/storage/lifetime.rs — OK.  
- daemon/src/core/storage/providers/**/* — OK (data access only).  
- daemon/src/core/storage/rocksdb/**/* — OK.  
- daemon/src/core/storage/sled/**/* — OK.  
- daemon/src/core/storage/providers/versioned/**/* — OK.  
- daemon/src/core/storage/providers/contract/**/* — OK.  
- daemon/src/core/state/chain_state/{mod.rs,storage.rs,apply.rs} — OK.  
- daemon/src/core/storage/rocksdb/types/**/* — OK.  
- daemon/src/core/storage/rocksdb/snapshot.rs — OK.  
- daemon/src/core/storage/rocksdb/column.rs — OK.  
- daemon/src/core/storage/sled/snapshot.rs — OK.  
- daemon/src/core/storage/sled/migrations.rs — OK.  

### P2P / networking
- daemon/src/p2p/mod.rs — OK.  
- daemon/src/p2p/connection.rs — OK.  
- daemon/src/p2p/compact_block_cache.rs — OK.  
- daemon/src/p2p/packet/**/* — OK (serialization bounds).  
- daemon/src/p2p/peer_list/**/* — OK.  
- daemon/src/p2p/tracker/**/* — OK.  
- daemon/src/p2p/diffie_hellman.rs — OK.  
- daemon/src/p2p/encryption.rs — OK.  
- daemon/src/p2p/error.rs — OK.  
- daemon/src/p2p/packet/bootstrap/**/* — OK.  
- daemon/src/p2p/chain_sync/bootstrap.rs — OK.  

### RPC / API
- daemon/src/rpc/{mod.rs,rpc.rs,getwork/mod.rs,getwork/miner.rs,websocket_wrapper.rs} — OK.  
- daemon/src/rpc/websocket/{mod.rs,security.rs} — OK.  
- common/src/rpc/**/* — OK.  
- wallet/src/api/**/* — OK.  
- wallet/src/api/server/**/* — OK.  

### Tako integration (VM/contract)
- daemon/src/tako_integration/**/* — OK (execution environment; not consensus rules).  

### Reachability
- daemon/src/core/reachability/**/* — OK (data structure; relies on ghostdag correctness).  

### Testing / benches / fuzz (non-production)
- daemon/src/core/tests/**/* — Test-only.  
- daemon/tests/**/* — Test-only.  
- testing-framework/**/* — Test-only.  
- daemon/benches/**/* — Benchmarks (non-production).  
- daemon/fuzz/fuzz_targets/**/* — Fuzz harness (non-production).  
- tests/api_tests.rs — Test-only.  
- wallet/tests/**/* — Test-only.  
- ai_miner/tests/**/* — Test-only.  
- testing-framework/tests/**/* — Test-only.  

### Wallet / Miner / AI miner (client-side)
- miner/src/{main.rs,config.rs} — OK (client).  
- ai_miner/src/**/* — OK (client).  
- wallet/src/**/* — OK (client).  
- genesis/src/**/* — OK (tooling).  
- wallet/precomputed_tables/**/* — OK (data).  

### Common library (non-consensus)
- common/src/{lib.rs,config.rs,network.rs,time.rs,utils.rs,queue.rs,immutable.rs,versioned_type.rs,context.rs} — OK.  
- common/src/account/**/* — OK.  
- common/src/api/**/* — OK.  
- common/src/ai_mining/**/* — OK.  
- common/src/serializer/**/* — OK.  
- common/src/crypto/**/* — OK (primitives; not consensus rules).  
- common/src/transaction/**/* — OK (verification logic consistent with consensus data).  
- common/src/contract/**/* — OK.  
- common/src/varuint.rs — OK.  
- common/src/tokio/**/* — OK.  
- common/src/prompt/**/* — OK.  
- common/build.rs — OK.  
- common/tests/security/crypto_security_tests.rs — Test-only.  

### Misc / examples
- daemon/src/doc_test_helpers.rs, common/src/doc_test_helpers.rs — OK.  
- daemon/examples/test_tako_jit.rs — Example.  
- testing-framework/examples/**/* — Example.  
- common/src/prompt/art.rs — OK.  
- common/src/api/data.rs — OK.  
