# TOS Error Codes Specification

> Based on actual implementation in `daemon/src/core/error.rs`

## 1. Overview

TOS uses the `BlockchainError` enum for all blockchain-related errors. Errors are converted to RPC error codes using enum discriminants.

**File**: `daemon/src/core/error.rs` (lines 151-499)

**Total Error Variants**: 118

## 2. RPC Error Code Mapping

**File**: `daemon/src/core/error.rs:507-512`

```rust
// RPC error code = 200 + enum_discriminant
CustomAny(200 + discriminant, error_message)
```

## 3. Error Categories

### 3.1 Core Blockchain Errors (Lines 153-317)

| Line | Error | Description |
|------|-------|-------------|
| 153 | `InvalidConfig` | Invalid configuration provided |
| 155 | `CorruptedData` | Invalid data on disk: corrupted |
| 165 | `CommitPointAlreadyStarted` | Commit point already started |
| 167 | `CommitPointNotStarted` | Commit point not started |
| 173 | `BlockNotOrdered` | Block is not ordered |
| 201 | `InvalidDifficulty` | Invalid difficulty |
| 251 | `BlockNotFound(Hash)` | Block not found: {hash} |
| 253 | `BlockHeightNotFound(u64)` | Block height not found: {height} |
| 255 | `LowerCumulativeDifficulty` | Chain has too low cumulative difficulty |
| 257 | `NoCumulativeDifficulty` | No cumulative difficulty found |
| 271 | `IsSyncing` | Blockchain is syncing |
| 279 | `NotEnoughBlocks` | Not enough blocks |
| 281 | `Unknown` | Unknown data store error |
| 293 | `NotFoundOnDisk(DiskContext)` | Data not found on disk |
| 305 | `InvalidBlockVersion` | Invalid block version |
| 313 | `AlreadyInChain` | Block is already in chain |
| 315 | `InvalidReachability` | Block has invalid reachability |
| 317 | `BlockDeviation` | Block has too much deviated |
| 319 | `InvalidGenesisHash` | Invalid genesis block hash |

### 3.2 Block Validation Errors

| Line | Error | Description |
|------|-------|-------------|
| 163 | `InvalidTipsOrder(Hash, Hash, Hash)` | Invalid tip order for block |
| 180 | `InvalidBalancesMerkleHash(...)` | Invalid balances merkle hash |
| 187 | `InvalidTipsMerkleHash(...)` | Invalid tips merkle hash |
| 191 | `TimestampIsLessThanParent(...)` | Timestamp less than parent |
| 193 | `TimestampIsInFuture(...)` | Timestamp in future |
| 195 | `InvalidBlockHeight(u64, u64)` | Block height mismatch |
| 197 | `BlockHeightZeroNotAllowed` | Block height zero not allowed |
| 199 | `InvalidBlockHeightStableHeight` | Block height in stable range |
| 209 | `InvalidBlockSize(usize, usize)` | Block size exceeds limit |
| 211 | `InvalidBlockTxs(usize, usize)` | Invalid TX count in block |
| 213 | `InvalidTxInBlock(Hash)` | Unknown TX in block |
| 297 | `ExpectedTips` | Expected at least one tip |
| 299 | `InvalidTipsCount(Hash, usize)` | Invalid tips count |
| 301 | `InvalidTipsNotFound(Hash, Hash)` | Tip not found in chain |
| 303 | `InvalidTipsDifficulty(Hash, Hash)` | Invalid tips difficulty |
| 307 | `MissingVrfData(Hash)` | Block missing VRF data |
| 309 | `InvalidVrfData(Hash, String)` | Invalid VRF data |

### 3.3 Transaction Errors

| Line | Error | Description |
|------|-------|-------------|
| 189 | `TxTooBig(usize, usize)` | Transaction size exceeds limit |
| 215 | `TxNotFound(Hash)` | TX not found in mempool |
| 217 | `TxNotFoundInSortedList(Hash)` | TX in mempool but not sorted |
| 219 | `TxAlreadyInMempool(Hash)` | TX already in mempool |
| 221 | `TxEmpty(Hash)` | Normal TX is empty |
| 233 | `TxAlreadyInBlock(Hash)` | TX already in block |
| 273 | `InvalidTransactionSignature` | Invalid transaction signature |
| 283 | `NoTxSignature` | No signature found for TX |
| 311 | `InvalidTxVersion` | Invalid TX version |
| 349 | `TxAlreadyInBlockchain(Hash)` | TX already in blockchain |
| 379 | `TransferCount` | Invalid transfer count |
| 389 | `InvalidTransactionFormat` | Invalid transaction format |
| 399 | `InvalidTransactionMultiThread` | Invalid TX in multi-thread verification |

### 3.4 Nonce Errors

| Line | Error | Description |
|------|-------|-------------|
| 203 | `TxNonceAlreadyUsed(Nonce, Hash)` | Nonce already used by TX |
| 241 | `InvalidTransactionNonce(Nonce, Nonce)` | Invalid TX nonce |
| 321 | `InvalidTxNonce(Hash, Nonce, Nonce, Address)` | Detailed nonce error |
| 323 | `InvalidTxNonceMempoolCache(...)` | Nonce outside mempool range |
| 371 | `InvalidNonce(Hash, Nonce, Nonce)` | Invalid nonce for TX |

### 3.5 Balance and Fee Errors

| Line | Error | Description |
|------|-------|-------------|
| 229 | `NoPreviousBalanceFound` | No previous balance found |
| 235 | `InvalidTxFee(u64, u64)` | Invalid TX fee |
| 237 | `FeesToLowToOverride(u64, u64)` | Fee too low to override |
| 329 | `NoBalance(Address)` | No balance found for address |
| 331 | `NoBalanceChanges(...)` | No balance changes |
| 333 | `NoNonce(Address)` | No nonce found for address |
| 335 | `Overflow` | Overflow detected |
| 337 | `BalanceOverflow` | Balance overflow (> u64::MAX) |

### 3.6 Reference Errors

| Line | Error | Description |
|------|-------|-------------|
| 223 | `InvalidReferenceHash` | Invalid reference block hash |
| 227 | `InvalidReferenceTopoheight(...)` | Invalid reference topoheight |
| 231 | `NoStableReferenceFound` | No stable reference found |

### 3.7 Account Errors

| Line | Error | Description |
|------|-------|-------------|
| 239 | `AccountNotFound(Address)` | Account not found |
| 243 | `InvalidTransactionToSender(Hash)` | Sender cannot send to self |
| 365 | `NoTxSender(Address)` | TX sender account not found |
| 373 | `SenderIsReceiver` | Sender cannot be receiver |
| 401 | `UnknownAccount` | Unknown account |

### 3.8 Contract Errors

| Line | Error | Description |
|------|-------|-------------|
| 157 | `NoContractBalance` | No contract balance found |
| 159 | `ContractAlreadyExists` | Contract already exists |
| 161 | `ContractNotFound(Hash)` | Contract not found |
| 285 | `SmartContractTodo` | Smart contract not supported |
| 391 | `InvalidInvokeContract` | Invalid invoke contract |

### 3.9 Asset Errors

| Line | Error | Description |
|------|-------|-------------|
| 325 | `AssetNotFound(Hash)` | Invalid asset ID |
| 381 | `Commitments` | Invalid commitments assets |

### 3.10 MultiSig Errors

| Line | Error | Description |
|------|-------|-------------|
| 169 | `NoMultisig` | No multisig found |
| 383 | `MultiSigNotConfigured` | MultiSig not configured |
| 385 | `MultiSigParticipants` | Invalid multisig participants |
| 387 | `MultiSigThreshold` | Invalid multisig threshold |
| 395 | `MultiSigNotFound` | MultiSig not found |

### 3.11 Referral System Errors (Lines 406-416)

| Line | Error | Description |
|------|-------|-------------|
| 406 | `ReferralAlreadyBound` | User already bound referrer |
| 408 | `ReferralSelfReferral` | Cannot self-refer |
| 410 | `ReferralCircularReference` | Circular reference detected |
| 412 | `ReferralRatiosTooHigh` | Reward ratio > 100% |
| 414 | `ReferralReferrerNotFound` | Referrer not found |
| 416 | `ReferralRecordNotFound` | Referral record not found |

### 3.12 KYC System Errors (Lines 422-438)

| Line | Error | Description |
|------|-------|-------------|
| 422 | `KycNotFound` | KYC record not found |
| 424 | `KycAlreadySet` | KYC already set |
| 426 | `InvalidKycLevel` | Invalid KYC level |
| 428 | `KycExpired` | KYC has expired |
| 430 | `KycRevoked` | KYC is revoked |
| 432 | `KycSuspended` | KYC is suspended |
| 434 | `InsufficientKycLevel(u16, u16)` | Insufficient KYC level |
| 436 | `KycDowngradeNotAllowed(...)` | KYC downgrade not allowed |
| 438 | `KycLevelExceedsCommitteeMax` | KYC level exceeds max |

### 3.13 Committee System Errors (Lines 442-466)

| Line | Error | Description |
|------|-------|-------------|
| 442 | `CommitteeNotFound` | Committee not found |
| 444 | `CommitteeAlreadyExists` | Committee already exists |
| 446 | `GlobalCommitteeAlreadyExists` | Global committee exists |
| 448 | `GlobalCommitteeNotBootstrapped` | Global committee not bootstrapped |
| 450 | `ParentCommitteeNotFound` | Parent committee not found |
| 452 | `InvalidMaxKycLevel` | Invalid max KYC level |
| 454 | `MemberNotFound` | Member not found |
| 456 | `MemberAlreadyExists` | Member already exists |
| 458 | `CannotRemoveLastMember` | Cannot remove last member |
| 460 | `InsufficientApprovals` | Insufficient approvals |
| 462 | `CommitteeNotActive` | Committee not active |
| 464 | `InvalidCommitteeThreshold` | Invalid threshold |
| 466 | `CommitteeHasChildren` | Cannot delete with children |

### 3.14 TNS (TOS Name Service) Errors (Lines 474-494)

| Line | Error | Description |
|------|-------|-------------|
| 474 | `TnsNameAlreadyRegistered` | Name already registered |
| 476 | `TnsAccountAlreadyHasName` | Account already has name |
| 478 | `TnsNameNotFound` | Name not found |
| 480 | `TnsMessageIdAlreadyUsed` | Message ID replay attack |
| 482 | `TnsSenderNotNameOwner` | Sender not name owner |
| 484 | `TnsRecipientNotRegistered` | Recipient not registered |
| 486 | `TnsInvalidTtl` | Invalid TTL value |
| 488 | `TnsMessageTooLarge` | Message too large |
| 490 | `TnsInvalidNameFormat` | Invalid name format |
| 492 | `TnsNameReserved` | Name is reserved |

### 3.15 Privacy (UNO) Errors

| Line | Error | Description |
|------|-------|-------------|
| 470 | `UnoNotImplemented` | UNO not implemented yet |
| 361 | `InvalidCiphertext` | Invalid ciphertext |
| 363 | `NoSenderOutput` | No sender output |

### 3.16 Pruning Errors (Lines 351-357)

| Line | Error | Description |
|------|-------|-------------|
| 351 | `PruneHeightTooHigh` | Cannot prune, not enough blocks |
| 353 | `PruneZero` | Cannot prune to 0 |
| 355 | `PruneLowerThanLastPruned` | Prune height too low |
| 357 | `AutoPruneMode` | Auto prune misconfigured |

### 3.17 Proof Errors

| Line | Error | Description |
|------|-------|-------------|
| 375 | `TransactionProof(...)` | Invalid transaction proof |
| 377 | `POWHashError(...)` | Error generating PoW hash |

### 3.18 System Errors

| Line | Error | Description |
|------|-------|-------------|
| 259 | `ErrorStd(io::Error)` | Standard I/O error |
| 261 | `ErrorOnBech32(...)` | Bech32 encoding error |
| 263 | `ErrorOnP2p(...)` | P2P error |
| 265 | `ErrorOnReader(...)` | Reader error |
| 269 | `PoisonError(String)` | Lock poison error |
| 289 | `DatabaseError(sled::Error)` | Database error |
| 291 | `UnsupportedOperation` | Unsupported operation |
| 295 | `ConfigSyncMode` | Invalid sync mode config |
| 397 | `ModuleError(String)` | Module error |
| 418 | `NotImplemented` | Feature not implemented |
| 494 | `InvalidPublicKey` | Invalid public key |
| 498 | `Storage(StorageError)` | Storage error |

## 4. Usage in Verification

**File**: `common/src/transaction/verify/error.rs`

Verification errors are separate from blockchain errors and used during TX validation.

## 5. Error Conversion

Errors can be converted from various sources:

```rust
#[derive(Error, Debug)]
pub enum BlockchainError {
    #[from] std::io::Error,
    #[from] Bech32Error,
    #[from] P2pError,
    #[from] ReaderError,
    #[from] sled::Error,
    #[from] anyhow::Error,
    // ... etc
}
```

---

*Document Version: 1.0*
*Based on: TOS Rust implementation*
*Last Updated: 2026-02-04*
