// KYC Transaction Payloads
// This module defines transaction types for KYC operations
//
// Transaction Types:
// - SetKyc: Set user KYC level (committee approval)
// - RevokeKyc: Revoke user KYC (committee approval)
// - RenewKyc: Renew expiring KYC (committee approval)
// - TransferKyc: Transfer KYC across regions (dual committee approval)
// - BootstrapCommittee: Create Global Committee (one-time, BOOTSTRAP_ADDRESS)
// - RegisterCommittee: Create regional committee (parent committee approval)
// - UpdateCommittee: Modify committee configuration (committee approval)
// - EmergencySuspend: Fast-track KYC suspension (2 members, 24h timeout)
//
// Gas Costs:
// - SetKyc: 50,000 gas
// - RevokeKyc: 30,000 gas
// - RenewKyc: 30,000 gas
// - TransferKyc: 60,000 gas
// - BootstrapCommittee: 100,000 gas
// - RegisterCommittee: 80,000 gas
// - UpdateCommittee: 40,000 gas
// - EmergencySuspend: 20,000 gas
//
// Reference: TOS-KYC-Level-Design.md

mod bootstrap;
mod register;
mod revoke;
mod set_kyc;
mod transfer;
mod update;

pub use bootstrap::*;
pub use register::*;
pub use revoke::*;
pub use set_kyc::*;
pub use transfer::*;
pub use update::*;
