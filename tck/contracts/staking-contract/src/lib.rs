//! # Staking Contract
//!
//! A staking contract that allows users to stake tokens and earn rewards.

#![no_std]
#![no_main]

use tako_sdk::*;

pub type Address = [u8; 32];

// Storage keys
const KEY_OWNER: &[u8] = b"owner";
const KEY_REWARD_RATE: &[u8] = b"rr"; // rewards per second per staked token
const KEY_TOTAL_STAKED: &[u8] = b"ts";
const KEY_LAST_UPDATE: &[u8] = b"lu";
const KEY_REWARD_PER_TOKEN: &[u8] = b"rpt";
const KEY_STAKE_PREFIX: &[u8] = b"stk:"; // stk:{address} -> amount, reward_debt, rewards
const KEY_USER_RPT_PREFIX: &[u8] = b"urpt:"; // urpt:{address} -> user reward per token snapshot

// Error codes
define_errors! {
    OnlyOwner = 1601,
    ZeroAmount = 1602,
    InsufficientStake = 1603,
    NoRewardsToClaim = 1604,
    StorageError = 1605,
    InvalidInput = 1606,
}

entrypoint!(process_instruction);

fn process_instruction(input: &[u8]) -> entrypoint::Result<()> {
    if input.is_empty() {
        return Ok(());
    }

    match input[0] {
        // Init: [0, owner[32], reward_rate[8]]
        0 => {
            if input.len() < 41 {
                return Err(InvalidInput);
            }
            let mut owner = [0u8; 32];
            owner.copy_from_slice(&input[1..33]);
            let reward_rate = u64::from_le_bytes(input[33..41].try_into().unwrap());
            init(&owner, reward_rate)
        }
        // Stake: [1, user[32], amount[8], current_time[8]]
        1 => {
            if input.len() < 49 {
                return Err(InvalidInput);
            }
            let mut user = [0u8; 32];
            user.copy_from_slice(&input[1..33]);
            let amount = u64::from_le_bytes(input[33..41].try_into().unwrap());
            let current_time = u64::from_le_bytes(input[41..49].try_into().unwrap());
            stake(&user, amount, current_time)
        }
        // Unstake: [2, user[32], amount[8], current_time[8]]
        2 => {
            if input.len() < 49 {
                return Err(InvalidInput);
            }
            let mut user = [0u8; 32];
            user.copy_from_slice(&input[1..33]);
            let amount = u64::from_le_bytes(input[33..41].try_into().unwrap());
            let current_time = u64::from_le_bytes(input[41..49].try_into().unwrap());
            unstake(&user, amount, current_time)
        }
        // Claim rewards: [3, user[32], current_time[8]]
        3 => {
            if input.len() < 41 {
                return Err(InvalidInput);
            }
            let mut user = [0u8; 32];
            user.copy_from_slice(&input[1..33]);
            let current_time = u64::from_le_bytes(input[33..41].try_into().unwrap());
            claim_rewards(&user, current_time)
        }
        // Set reward rate: [4, caller[32], new_rate[8], current_time[8]]
        4 => {
            if input.len() < 49 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            let new_rate = u64::from_le_bytes(input[33..41].try_into().unwrap());
            let current_time = u64::from_le_bytes(input[41..49].try_into().unwrap());
            set_reward_rate(&caller, new_rate, current_time)
        }
        // Get staked amount: [5, user[32]]
        5 => {
            if input.len() < 33 {
                return Err(InvalidInput);
            }
            let mut user = [0u8; 32];
            user.copy_from_slice(&input[1..33]);
            let staked = read_stake(&user);
            set_return_data(&staked.to_le_bytes());
            Ok(())
        }
        _ => Ok(()),
    }
}

fn init(owner: &Address, reward_rate: u64) -> entrypoint::Result<()> {
    storage_write(KEY_OWNER, owner).map_err(|_| StorageError)?;
    storage_write(KEY_REWARD_RATE, &reward_rate.to_le_bytes()).map_err(|_| StorageError)?;
    storage_write(KEY_TOTAL_STAKED, &0u64.to_le_bytes()).map_err(|_| StorageError)?;
    storage_write(KEY_LAST_UPDATE, &0u64.to_le_bytes()).map_err(|_| StorageError)?;
    storage_write(KEY_REWARD_PER_TOKEN, &0u64.to_le_bytes()).map_err(|_| StorageError)?;
    log("Staking contract initialized");
    Ok(())
}

fn stake(user: &Address, amount: u64, current_time: u64) -> entrypoint::Result<()> {
    if amount == 0 {
        return Err(ZeroAmount);
    }

    // Update global reward state
    update_reward_per_token(current_time)?;

    // Update user rewards
    update_user_rewards(user)?;

    // Update stake
    let current_stake = read_stake(user);
    let stake_key = make_stake_key(user);
    storage_write(&stake_key, &(current_stake + amount).to_le_bytes()).map_err(|_| StorageError)?;

    // Update total staked
    let total_staked = read_u64(KEY_TOTAL_STAKED);
    storage_write(KEY_TOTAL_STAKED, &(total_staked + amount).to_le_bytes())
        .map_err(|_| StorageError)?;

    log("Tokens staked");
    Ok(())
}

fn unstake(user: &Address, amount: u64, current_time: u64) -> entrypoint::Result<()> {
    if amount == 0 {
        return Err(ZeroAmount);
    }

    let current_stake = read_stake(user);
    if current_stake < amount {
        return Err(InsufficientStake);
    }

    // Update global reward state
    update_reward_per_token(current_time)?;

    // Update user rewards
    update_user_rewards(user)?;

    // Update stake
    let stake_key = make_stake_key(user);
    storage_write(&stake_key, &(current_stake - amount).to_le_bytes()).map_err(|_| StorageError)?;

    // Update total staked
    let total_staked = read_u64(KEY_TOTAL_STAKED);
    storage_write(
        KEY_TOTAL_STAKED,
        &total_staked.saturating_sub(amount).to_le_bytes(),
    )
    .map_err(|_| StorageError)?;

    log("Tokens unstaked");
    Ok(())
}

fn claim_rewards(user: &Address, current_time: u64) -> entrypoint::Result<()> {
    // Update global reward state
    update_reward_per_token(current_time)?;

    // Update user rewards
    update_user_rewards(user)?;

    // Get pending rewards (stored separately)
    let rewards = read_user_pending_rewards(user);
    if rewards == 0 {
        return Err(NoRewardsToClaim);
    }

    // Clear pending rewards
    let rewards_key = make_user_rewards_key(user);
    storage_write(&rewards_key, &0u64.to_le_bytes()).map_err(|_| StorageError)?;

    // Return claimed amount
    set_return_data(&rewards.to_le_bytes());
    log("Rewards claimed");
    Ok(())
}

fn set_reward_rate(caller: &Address, new_rate: u64, current_time: u64) -> entrypoint::Result<()> {
    let owner = read_owner()?;
    if *caller != owner {
        return Err(OnlyOwner);
    }

    // Update rewards with old rate first
    update_reward_per_token(current_time)?;

    // Set new rate
    storage_write(KEY_REWARD_RATE, &new_rate.to_le_bytes()).map_err(|_| StorageError)?;
    log("Reward rate updated");
    Ok(())
}

fn update_reward_per_token(current_time: u64) -> entrypoint::Result<()> {
    let total_staked = read_u64(KEY_TOTAL_STAKED);
    let last_update = read_u64(KEY_LAST_UPDATE);
    let reward_per_token = read_u64(KEY_REWARD_PER_TOKEN);
    let reward_rate = read_u64(KEY_REWARD_RATE);

    if total_staked > 0 && current_time > last_update {
        let time_delta = current_time - last_update;
        let rewards = time_delta * reward_rate;
        let new_rpt = reward_per_token + (rewards * 1_000_000 / total_staked);
        storage_write(KEY_REWARD_PER_TOKEN, &new_rpt.to_le_bytes()).map_err(|_| StorageError)?;
    }

    storage_write(KEY_LAST_UPDATE, &current_time.to_le_bytes()).map_err(|_| StorageError)?;
    Ok(())
}

fn update_user_rewards(user: &Address) -> entrypoint::Result<()> {
    let user_stake = read_stake(user);
    if user_stake == 0 {
        return Ok(());
    }

    let reward_per_token = read_u64(KEY_REWARD_PER_TOKEN);
    let user_rpt = read_user_rpt(user);

    if reward_per_token > user_rpt {
        let pending = user_stake * (reward_per_token - user_rpt) / 1_000_000;
        let current_rewards = read_user_pending_rewards(user);

        let rewards_key = make_user_rewards_key(user);
        storage_write(&rewards_key, &(current_rewards + pending).to_le_bytes())
            .map_err(|_| StorageError)?;
    }

    // Update user's reward per token snapshot
    let user_rpt_key = make_user_rpt_key(user);
    storage_write(&user_rpt_key, &reward_per_token.to_le_bytes()).map_err(|_| StorageError)?;

    Ok(())
}

// Helper functions

fn read_owner() -> entrypoint::Result<Address> {
    let mut buffer = [0u8; 32];
    let len = storage_read(KEY_OWNER, &mut buffer);
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

fn read_stake(user: &Address) -> u64 {
    let key = make_stake_key(user);
    read_u64(&key)
}

fn read_user_rpt(user: &Address) -> u64 {
    let key = make_user_rpt_key(user);
    read_u64(&key)
}

fn read_user_pending_rewards(user: &Address) -> u64 {
    let key = make_user_rewards_key(user);
    read_u64(&key)
}

fn make_stake_key(user: &Address) -> [u8; 36] {
    let mut key = [0u8; 36];
    key[0..4].copy_from_slice(KEY_STAKE_PREFIX);
    key[4..36].copy_from_slice(user);
    key
}

fn make_user_rpt_key(user: &Address) -> [u8; 37] {
    let mut key = [0u8; 37];
    key[0..5].copy_from_slice(KEY_USER_RPT_PREFIX);
    key[5..37].copy_from_slice(user);
    key
}

fn make_user_rewards_key(user: &Address) -> [u8; 36] {
    let mut key = [0u8; 36];
    key[0..4].copy_from_slice(b"rwd:");
    key[4..36].copy_from_slice(user);
    key
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
