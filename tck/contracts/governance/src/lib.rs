//! # Governance Contract
//!
//! A decentralized governance system with proposal creation, voting, and execution.

#![no_std]
#![no_main]

use tako_sdk::*;

/// Address type
pub type Address = [u8; 32];
pub type ProposalId = [u8; 32];

// Proposal status
const STATUS_ACTIVE: u8 = 1;
const STATUS_SUCCEEDED: u8 = 2;
const STATUS_DEFEATED: u8 = 3;
const STATUS_EXECUTED: u8 = 4;
const STATUS_CANCELLED: u8 = 5;

// Storage keys
const KEY_ADMIN: &[u8] = b"admin";
const KEY_QUORUM: &[u8] = b"quorum";
const KEY_VOTING_PERIOD: &[u8] = b"vp";
const KEY_PROPOSAL_COUNT: &[u8] = b"pc";
const KEY_VOTING_POWER_PREFIX: &[u8] = b"pow:";
const KEY_PROPOSAL_PREFIX: &[u8] = b"prop:"; // prop:{id} -> status, votes_for, votes_against, end_time, proposer
const KEY_VOTE_PREFIX: &[u8] = b"vote:"; // vote:{proposal_id}{voter} -> 1 (voted)

// Error codes
define_errors! {
    OnlyAdmin = 1401,
    ProposalNotFound = 1402,
    VotingNotStarted = 1403,
    VotingEnded = 1404,
    AlreadyVoted = 1405,
    InsufficientVotingPower = 1406,
    CannotExecute = 1407,
    NotAuthorized = 1408,
    StorageError = 1409,
    InvalidInput = 1410,
}

entrypoint!(process_instruction);

fn process_instruction(input: &[u8]) -> entrypoint::Result<()> {
    if input.is_empty() {
        return Ok(());
    }

    match input[0] {
        // Init: [0, admin[32], quorum[8], voting_period[8]]
        0 => {
            if input.len() < 49 {
                return Err(InvalidInput);
            }
            let mut admin = [0u8; 32];
            admin.copy_from_slice(&input[1..33]);
            let quorum = u64::from_le_bytes(input[33..41].try_into().unwrap());
            let voting_period = u64::from_le_bytes(input[41..49].try_into().unwrap());
            init(&admin, quorum, voting_period)
        }
        // Set voting power: [1, caller[32], user[32], power[8]]
        1 => {
            if input.len() < 73 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            let mut user = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            user.copy_from_slice(&input[33..65]);
            let power = u64::from_le_bytes(input[65..73].try_into().unwrap());
            set_voting_power(&caller, &user, power)
        }
        // Create proposal: [2, proposer[32], current_time[8]]
        2 => {
            if input.len() < 41 {
                return Err(InvalidInput);
            }
            let mut proposer = [0u8; 32];
            proposer.copy_from_slice(&input[1..33]);
            let current_time = u64::from_le_bytes(input[33..41].try_into().unwrap());
            create_proposal(&proposer, current_time)
        }
        // Vote: [3, voter[32], proposal_id[32], support[1], current_time[8]]
        3 => {
            if input.len() < 74 {
                return Err(InvalidInput);
            }
            let mut voter = [0u8; 32];
            let mut proposal_id = [0u8; 32];
            voter.copy_from_slice(&input[1..33]);
            proposal_id.copy_from_slice(&input[33..65]);
            let support = input[65] != 0;
            let current_time = u64::from_le_bytes(input[66..74].try_into().unwrap());
            vote(&voter, &proposal_id, support, current_time)
        }
        // Execute: [4, caller[32], proposal_id[32], current_time[8]]
        4 => {
            if input.len() < 73 {
                return Err(InvalidInput);
            }
            let mut proposal_id = [0u8; 32];
            proposal_id.copy_from_slice(&input[33..65]);
            let current_time = u64::from_le_bytes(input[65..73].try_into().unwrap());
            execute_proposal(&proposal_id, current_time)
        }
        // Cancel: [5, caller[32], proposal_id[32]]
        5 => {
            if input.len() < 65 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            let mut proposal_id = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            proposal_id.copy_from_slice(&input[33..65]);
            cancel_proposal(&caller, &proposal_id)
        }
        _ => Ok(()),
    }
}

fn init(admin: &Address, quorum: u64, voting_period: u64) -> entrypoint::Result<()> {
    storage_write(KEY_ADMIN, admin).map_err(|_| StorageError)?;
    storage_write(KEY_QUORUM, &quorum.to_le_bytes()).map_err(|_| StorageError)?;
    storage_write(KEY_VOTING_PERIOD, &voting_period.to_le_bytes()).map_err(|_| StorageError)?;
    storage_write(KEY_PROPOSAL_COUNT, &0u64.to_le_bytes()).map_err(|_| StorageError)?;
    log("Governance initialized");
    Ok(())
}

fn set_voting_power(caller: &Address, user: &Address, power: u64) -> entrypoint::Result<()> {
    let admin = read_admin()?;
    if *caller != admin {
        return Err(OnlyAdmin);
    }

    let key = make_voting_power_key(user);
    storage_write(&key, &power.to_le_bytes()).map_err(|_| StorageError)?;
    log("Voting power set");
    Ok(())
}

fn create_proposal(proposer: &Address, current_time: u64) -> entrypoint::Result<()> {
    let count = read_u64(KEY_PROPOSAL_COUNT);
    let voting_period = read_u64(KEY_VOTING_PERIOD);

    // Generate proposal ID using proposer and count
    let mut proposal_id = [0u8; 32];
    for i in 0..32 {
        proposal_id[i] = proposer[i];
    }
    proposal_id[0] ^= (count & 0xFF) as u8;
    proposal_id[1] ^= ((count >> 8) & 0xFF) as u8;

    // Store proposal data: [status(1), votes_for(8), votes_against(8), end_time(8), proposer(32)]
    let mut proposal_data = [0u8; 57];
    proposal_data[0] = STATUS_ACTIVE;
    // votes_for starts at 0 (bytes 1-8)
    // votes_against starts at 0 (bytes 9-16)
    let end_time = current_time + voting_period;
    proposal_data[17..25].copy_from_slice(&end_time.to_le_bytes());
    proposal_data[25..57].copy_from_slice(proposer);

    let key = make_proposal_key(&proposal_id);
    storage_write(&key, &proposal_data).map_err(|_| StorageError)?;

    // Update count
    storage_write(KEY_PROPOSAL_COUNT, &(count + 1).to_le_bytes()).map_err(|_| StorageError)?;

    set_return_data(&proposal_id);
    log("Proposal created");
    Ok(())
}

fn vote(
    voter: &Address,
    proposal_id: &ProposalId,
    support: bool,
    current_time: u64,
) -> entrypoint::Result<()> {
    // Read proposal
    let proposal_key = make_proposal_key(proposal_id);
    let mut proposal_data = [0u8; 57];
    let len = storage_read(&proposal_key, &mut proposal_data);
    if len != 57 {
        return Err(ProposalNotFound);
    }

    // Check status
    if proposal_data[0] != STATUS_ACTIVE {
        return Err(VotingEnded);
    }

    // Check voting period
    let end_time = u64::from_le_bytes(proposal_data[17..25].try_into().unwrap());
    if current_time >= end_time {
        return Err(VotingEnded);
    }

    // Check if already voted
    let vote_key = make_vote_key(proposal_id, voter);
    let mut vote_buffer = [0u8; 1];
    if storage_read(&vote_key, &mut vote_buffer) > 0 {
        return Err(AlreadyVoted);
    }

    // Get voting power
    let voting_power = read_voting_power(voter);
    if voting_power == 0 {
        return Err(InsufficientVotingPower);
    }

    // Update votes
    if support {
        let votes_for = u64::from_le_bytes(proposal_data[1..9].try_into().unwrap());
        proposal_data[1..9].copy_from_slice(&(votes_for + voting_power).to_le_bytes());
    } else {
        let votes_against = u64::from_le_bytes(proposal_data[9..17].try_into().unwrap());
        proposal_data[9..17].copy_from_slice(&(votes_against + voting_power).to_le_bytes());
    }

    // Save proposal and vote record
    storage_write(&proposal_key, &proposal_data).map_err(|_| StorageError)?;
    storage_write(&vote_key, &[1u8]).map_err(|_| StorageError)?;

    log("Vote recorded");
    Ok(())
}

fn execute_proposal(proposal_id: &ProposalId, current_time: u64) -> entrypoint::Result<()> {
    let proposal_key = make_proposal_key(proposal_id);
    let mut proposal_data = [0u8; 57];
    let len = storage_read(&proposal_key, &mut proposal_data);
    if len != 57 {
        return Err(ProposalNotFound);
    }

    // Check voting ended
    let end_time = u64::from_le_bytes(proposal_data[17..25].try_into().unwrap());
    if current_time < end_time {
        return Err(VotingNotStarted);
    }

    // Check if succeeded
    let votes_for = u64::from_le_bytes(proposal_data[1..9].try_into().unwrap());
    let votes_against = u64::from_le_bytes(proposal_data[9..17].try_into().unwrap());
    let quorum = read_u64(KEY_QUORUM);

    if votes_for < quorum || votes_for <= votes_against {
        return Err(CannotExecute);
    }

    // Mark as executed
    proposal_data[0] = STATUS_EXECUTED;
    storage_write(&proposal_key, &proposal_data).map_err(|_| StorageError)?;

    log("Proposal executed");
    Ok(())
}

fn cancel_proposal(caller: &Address, proposal_id: &ProposalId) -> entrypoint::Result<()> {
    let proposal_key = make_proposal_key(proposal_id);
    let mut proposal_data = [0u8; 57];
    let len = storage_read(&proposal_key, &mut proposal_data);
    if len != 57 {
        return Err(ProposalNotFound);
    }

    // Check authorization
    let admin = read_admin()?;
    let mut proposer = [0u8; 32];
    proposer.copy_from_slice(&proposal_data[25..57]);
    if *caller != proposer && *caller != admin {
        return Err(NotAuthorized);
    }

    // Check not already executed
    if proposal_data[0] == STATUS_EXECUTED {
        return Err(CannotExecute);
    }

    // Mark as cancelled
    proposal_data[0] = STATUS_CANCELLED;
    storage_write(&proposal_key, &proposal_data).map_err(|_| StorageError)?;

    log("Proposal cancelled");
    Ok(())
}

// Helper functions

fn read_admin() -> entrypoint::Result<Address> {
    let mut buffer = [0u8; 32];
    let len = storage_read(KEY_ADMIN, &mut buffer);
    if len != 32 {
        return Err(StorageError);
    }
    Ok(buffer)
}

fn read_u64(key: &[u8]) -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(key, &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

fn read_voting_power(user: &Address) -> u64 {
    let key = make_voting_power_key(user);
    read_u64(&key)
}

fn make_voting_power_key(user: &Address) -> [u8; 36] {
    let mut key = [0u8; 36];
    key[0..4].copy_from_slice(KEY_VOTING_POWER_PREFIX);
    key[4..36].copy_from_slice(user);
    key
}

fn make_proposal_key(id: &ProposalId) -> [u8; 37] {
    let mut key = [0u8; 37];
    key[0..5].copy_from_slice(KEY_PROPOSAL_PREFIX);
    key[5..37].copy_from_slice(id);
    key
}

fn make_vote_key(proposal_id: &ProposalId, voter: &Address) -> [u8; 69] {
    let mut key = [0u8; 69];
    key[0..5].copy_from_slice(KEY_VOTE_PREFIX);
    key[5..37].copy_from_slice(proposal_id);
    key[37..69].copy_from_slice(voter);
    key
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
