// Allowing await_holding_lock for std::sync::Mutex - this is a pre-existing architectural pattern.
// The code uses synchronous Mutex from std library with async functions. While this could cause
// deadlocks in theory, the current usage pattern is safe as locks are held briefly.
// TODO: Consider migrating to async-aware mutexes (tokio::sync::Mutex) in future refactoring.
#![allow(clippy::await_holding_lock)]

use anyhow::{Context, Result};
use clap::Parser;
use indexmap::IndexSet;
#[allow(unused_imports)]
use log::{error, info, warn};
use std::{borrow::Cow, fs::File, io::Write, path::Path, sync::Arc};
use tos_common::{
    async_handler,
    config::{init, TOS_ASSET},
    contract::Module,
    crypto::{Address, Hash, Hashable, Signature, HASH_SIZE},
    network::Network,
    prompt::{
        argument::{Arg, ArgType, ArgumentManager},
        command::{Command, CommandError, CommandHandler, CommandManager},
        Color, Prompt, PromptConfig,
    },
    serializer::Serializer,
    tokio,
    transaction::{
        builder::{
            ContractDepositBuilder, DeployContractBuilder, EnergyBuilder, FeeBuilder,
            InvokeContractBuilder, MultiSigBuilder, TransactionTypeBuilder, TransferBuilder,
            UnoTransferBuilder,
        },
        multisig::{MultiSig, SignatureId},
        BurnPayload, DelegationEntry, MultiSigPayload, Transaction, TxVersion,
    },
    utils::{format_coin, format_tos, from_coin},
};
use tos_wallet::{
    config::{
        Config, JsonBatchConfig, LogProgressTableGenerationReportFunction, WalletCommand, DIR_PATH,
    },
    precomputed_tables,
    wallet::{RecoverOption, Wallet},
};

#[cfg(feature = "xswd")]
use {
    anyhow::Error,
    tos_common::{
        prompt::ShareablePrompt,
        rpc::RpcRequest,
        tokio::{spawn_task, sync::mpsc::UnboundedReceiver},
    },
    tos_wallet::{
        api::{AppStateShared, AuthConfig, PermissionResult},
        wallet::XSWDEvent,
    },
};

const ELEMENTS_PER_PAGE: usize = 10;

// ========== Helper Functions for Batch Mode ==========

/// Get a required argument from CLI (batch mode only)
#[allow(dead_code)]
fn get_required_arg(
    args: &mut ArgumentManager,
    name: &str,
    usage: &str,
) -> Result<String, CommandError> {
    if args.has_argument(name) {
        return Ok(args.get_value(name)?.to_string_value()?);
    }

    Err(CommandError::MissingRequiredArgument {
        arg: name.to_string(),
        usage: usage.to_string(),
    })
}

/// Get a required argument with usage example for better error messages (batch mode only)
fn get_required_arg_with_example(
    args: &mut ArgumentManager,
    name: &str,
    usage: &str,
    example: &str,
) -> Result<String, CommandError> {
    if args.has_argument(name) {
        return Ok(args.get_value(name)?.to_string_value()?);
    }

    Err(CommandError::MissingRequiredArgumentWithExample {
        arg: name.to_string(),
        usage: usage.to_string(),
        example: example.to_string(),
    })
}

/// Get an optional argument from CLI (batch mode only)
#[allow(dead_code)]
fn get_optional_arg(
    args: &mut ArgumentManager,
    name: &str,
) -> Result<Option<String>, CommandError> {
    if args.has_argument(name) {
        return Ok(Some(args.get_value(name)?.to_string_value()?));
    }
    Ok(None)
}

/// Get password from config with priority: CLI > File > Env > Error (batch mode only)
#[allow(dead_code)]
async fn get_password(config: &Config, _prompt: &Prompt) -> Result<String> {
    // Priority 1: CLI argument (least secure, warn in production)
    if let Some(pwd) = config.password.as_ref() {
        #[cfg(not(debug_assertions))]
        {
            if log::log_enabled!(log::Level::Warn) {
                warn!("Using --password in production is not secure. Consider --password-file or --password-from-env");
            }
        }
        return Ok(pwd.clone());
    }

    // Priority 2: Password file (recommended for automation)
    if let Some(file) = config.password_file.as_ref() {
        // Validate file exists and is readable
        let path = std::path::Path::new(file);
        if !path.exists() {
            return Err(anyhow::anyhow!("Password file not found: {file}"));
        }
        if !path.is_file() {
            return Err(anyhow::anyhow!("Password file path is not a file: {file}"));
        }

        let pwd = std::fs::read_to_string(file)
            .with_context(|| format!("Failed to read password file: {file}"))?;

        // Trim trailing newline (like geth)
        let pwd = pwd.trim_end_matches('\n').to_string();

        if pwd.is_empty() {
            return Err(anyhow::anyhow!("Password file is empty: {file}"));
        }

        // Validate password is not just whitespace
        if pwd.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "Password file contains only whitespace: {file}"
            ));
        }

        return Ok(pwd);
    }

    // Priority 3: Environment variable
    if config.password_from_env {
        return std::env::var("TOS_WALLET_PASSWORD")
            .context("Environment variable TOS_WALLET_PASSWORD not set");
    }

    // Priority 4: Error (batch mode only - no interactive prompt)
    Err(CommandError::PasswordRequired.into())
}

// SAFETY: Clippy false positive at line 468 - Ok(()) constructor does not use .expect()
// The disallowed_methods lint incorrectly flags the Ok(()) return value as using Result::expect
#[allow(clippy::disallowed_methods)]
#[tokio::main]
async fn main() -> Result<()> {
    init();

    let mut config: Config = Config::parse();
    if let Some(path) = config.config_file.as_ref() {
        if config.generate_config_template {
            if Path::new(path).exists() {
                eprintln!("Config file already exists at {path}");
                return Ok(());
            }

            let mut file = File::create(path).context("Error while creating config file")?;
            let json = serde_json::to_string_pretty(&config)
                .context("Error while serializing config file")?;
            file.write_all(json.as_bytes())
                .context("Error while writing config file")?;
            println!("Config file template generated at {path}");
            return Ok(());
        }

        let file = File::open(path).context("Error while opening config file")?;
        config = serde_json::from_reader(file).context("Error while reading config file")?;
    } else if config.generate_config_template {
        eprintln!(
            "Provided config file path is required to generate the template with --config-file"
        );
        return Ok(());
    }

    // Batch mode only - no interactive prompt loop
    let log_config = &config.log;
    let prompt = Prompt::new(PromptConfig {
        level: log_config.log_level,
        dir_path: &log_config.logs_path,
        filename_log: &log_config.filename_log,
        disable_file_logging: log_config.disable_file_logging,
        disable_file_log_date_based: log_config.disable_file_log_date_based,
        disable_colors: log_config.disable_log_color,
        enable_auto_compress_logs: log_config.auto_compress_logs,
        interactive: false, // Batch mode only
        module_logs: log_config.logs_modules.clone(),
        file_level: log_config.file_log_level.unwrap_or(log_config.log_level),
        show_ascii: !log_config.disable_ascii_art,
        logs_datetime_format: log_config.datetime_format.clone(),
    })?;

    #[cfg(feature = "api_server")]
    {
        // Sanity check
        // check that we don't have both server enabled
        if config.enable_xswd && config.rpc.rpc_bind_address.is_some() {
            error!("Invalid parameters configuration: RPC Server and XSWD cannot be enabled at the same time");
            return Ok(()); // exit
        }

        // check that username/password is not in param if bind address is not set
        if config.rpc.rpc_bind_address.is_none()
            && (config.rpc.rpc_password.is_some() || config.rpc.rpc_username.is_some())
        {
            error!("Invalid parameters configuration for rpc password and username: RPC Server is not enabled");
            return Ok(());
        }

        // check that username/password is set together if bind address is set
        if config.rpc.rpc_bind_address.is_some()
            && config.rpc.rpc_password.is_some() != config.rpc.rpc_username.is_some()
        {
            error!("Invalid parameters configuration: usernamd AND password must be provided");
            return Ok(());
        }
    }

    // Set batch mode based on command mode (not just exec mode)
    let command_manager =
        CommandManager::new_with_batch_mode(prompt.clone(), config.is_command_mode());
    command_manager.store_in_context(config.network)?;

    if let Some(path) = config.wallet_path.as_ref() {
        // Get password using our helper function
        // Priority: CLI > File > Env > Interactive > Error
        let password = get_password(&config, &prompt)
            .await
            .context("Failed to get wallet password")?;

        let precomputed_tables = precomputed_tables::read_or_generate_precomputed_tables(
            config.precomputed_tables.precomputed_tables_path.as_deref(),
            config.precomputed_tables.precomputed_tables_l1,
            LogProgressTableGenerationReportFunction,
            true,
        )
        .await?;
        let p = Path::new(path);
        let wallet_exists = p.exists() && p.is_dir() && Path::new(&format!("{path}/db")).exists();

        // Handle explicit 'create' subcommand
        if let Some(WalletCommand::Create) = config.command {
            if wallet_exists {
                anyhow::bail!(
                    "Wallet already exists at '{path}'. Use without 'create' subcommand to open it."
                );
            }
            if log::log_enabled!(log::Level::Info) {
                info!("Creating a new wallet at {path}");
            }
            // Determine recovery option: private_key takes precedence over seed
            let recover_option = if let Some(ref pk) = config.private_key {
                Some(RecoverOption::PrivateKey(pk.as_str()))
            } else {
                config.seed.as_deref().map(RecoverOption::Seed)
            };

            let wallet = Wallet::create(
                path,
                &password,
                recover_option,
                config.network,
                precomputed_tables,
                config.n_decryption_threads,
                config.network_concurrency,
            )
            .await?;

            // Display wallet info and exit
            println!("Wallet created successfully!");
            println!("Address: {}", wallet.get_address());
            // Only show seed if wallet was NOT recovered from private key
            if config.private_key.is_none() {
                if let Ok(seed) = wallet.get_seed(0) {
                    println!("\nSeed phrase (SAVE THIS SECURELY):");
                    println!("{}", seed);
                    println!("\nWARNING: Never share your seed phrase with anyone!");
                }
            }
            return Ok(());
        }

        // Default behavior: open existing or create new
        let wallet = if wallet_exists {
            if log::log_enabled!(log::Level::Info) {
                info!("Opening wallet {path}");
            }
            Wallet::open(
                path,
                &password,
                config.network,
                precomputed_tables,
                config.n_decryption_threads,
                config.network_concurrency,
            )?
        } else {
            if log::log_enabled!(log::Level::Info) {
                info!("Creating a new wallet at {path}");
            }
            // Determine recovery option: private_key takes precedence over seed
            let recover_option = if let Some(ref pk) = config.private_key {
                Some(RecoverOption::PrivateKey(pk.as_str()))
            } else {
                config.seed.as_deref().map(RecoverOption::Seed)
            };
            Wallet::create(
                path,
                &password,
                recover_option,
                config.network,
                precomputed_tables,
                config.n_decryption_threads,
                config.network_concurrency,
            )
            .await?
        };

        command_manager.register_default_commands()?;

        apply_config(
            config.clone(),
            &wallet,
            #[cfg(feature = "xswd")]
            &prompt,
        )
        .await;
        setup_wallet_command_manager(wallet, &command_manager).await?;

        // Batch mode: execute command and exit
        if let Some(json_str) = config.json.as_ref() {
            if log::log_enabled!(log::Level::Info) {
                info!("Executing batch command from JSON string");
            }
            execute_json_batch(&command_manager, json_str, &config).await?;
        } else if let Some(json_file) = config.json_file.as_ref() {
            if log::log_enabled!(log::Level::Info) {
                info!("Executing batch command from JSON file: {json_file}");
            }
            let json_content = std::fs::read_to_string(json_file)
                .with_context(|| format!("Failed to read JSON file: {json_file}"))?;
            execute_json_batch(&command_manager, &json_content, &config).await?;
        } else if let Some(cmd) = config.get_exec_command() {
            if log::log_enabled!(log::Level::Info) {
                info!("Executing command: {cmd}");
            }
            match command_manager.handle_command(cmd.clone()).await {
                Ok(_) => {
                    if log::log_enabled!(log::Level::Info) {
                        info!("Command executed successfully");
                    }
                }
                Err(e) => {
                    if log::log_enabled!(log::Level::Error) {
                        error!("Error executing command: {e:#}");
                    }
                    return Err(e.into());
                }
            }
        } else {
            // No command specified
            return Err(anyhow::anyhow!(
                "Batch mode requires one of: --exec, --json, or --json-file"
            ));
        }
    } else {
        // No wallet path provided
        register_default_commands(&command_manager).await?;

        // Batch mode without wallet: only allow certain commands (help, version, etc.)
        if let Some(cmd) = config.get_exec_command() {
            // Allow a few commands without wallet
            if cmd.starts_with("help") || cmd.starts_with("version") {
                if log::log_enabled!(log::Level::Info) {
                    info!("Executing command: {cmd}");
                }
                command_manager.handle_command(cmd.clone()).await?;
            } else {
                return Err(anyhow::anyhow!(
                    "Wallet path required for this command. Use --wallet-path <path>"
                ));
            }
        } else {
            return Err(anyhow::anyhow!(
                "Batch mode requires --wallet-path and one of: --exec, --json, or --json-file"
            ));
        }
    }

    // Close wallet if it was opened
    let wallet_opt = {
        command_manager
            .get_context()
            .lock()
            .ok()
            .and_then(|context| context.get::<Arc<Wallet>>().ok().cloned())
    };
    if let Some(wallet) = wallet_opt {
        wallet.close().await;
    }

    Ok(())
}

async fn register_default_commands(manager: &CommandManager) -> Result<(), CommandError> {
    manager.add_command(Command::with_optional_arguments(
        "open",
        "Open a wallet",
        vec![
            Arg::new("name", ArgType::String, "Wallet name to open"),
            Arg::new("password", ArgType::String, "Password to unlock the wallet"),
        ],
        CommandHandler::Async(async_handler!(open_wallet)),
    ))?;

    manager.add_command(Command::with_optional_arguments(
        "create",
        "Create a new wallet",
        vec![
            Arg::new("name", ArgType::String, "Name for the new wallet"),
            Arg::new(
                "password",
                ArgType::String,
                "Password to protect the wallet",
            ),
            Arg::new("confirm_password", ArgType::String, "Confirm the password"),
        ],
        CommandHandler::Async(async_handler!(create_wallet)),
    ))?;

    manager.add_command(Command::with_optional_arguments(
        "recover_seed",
        "Recover a wallet using a seed",
        vec![
            Arg::new("name", ArgType::String, "Name for the recovered wallet"),
            Arg::new(
                "password",
                ArgType::String,
                "Password to protect the wallet",
            ),
            Arg::new("seed", ArgType::String, "Recovery seed phrase"),
        ],
        CommandHandler::Async(async_handler!(recover_seed)),
    ))?;

    manager.add_command(Command::with_optional_arguments(
        "recover_private_key",
        "Recover a wallet using a private key",
        vec![
            Arg::new("name", ArgType::String, "Name for the recovered wallet"),
            Arg::new(
                "password",
                ArgType::String,
                "Password to protect the wallet",
            ),
            Arg::new("private_key", ArgType::String, "Private key for recovery"),
        ],
        CommandHandler::Async(async_handler!(recover_private_key)),
    ))?;

    manager.register_default_commands()?;

    Ok(())
}

fn unfreeze_tos_delegate_args() -> (Vec<Arg>, Vec<Arg>) {
    (
        vec![Arg::new(
            "amount",
            ArgType::String,
            "Amount of TOS to unfreeze",
        )],
        vec![
            Arg::new("delegatee", ArgType::String, "Delegatee address"),
            Arg::new(
                "record_index",
                ArgType::Number,
                "Delegation record index (required if multiple records exist)",
            ),
        ],
    )
}

#[cfg(feature = "xswd")]
// This must be run in a separate task
async fn xswd_handler(mut receiver: UnboundedReceiver<XSWDEvent>, prompt: ShareablePrompt) {
    while let Some(event) = receiver.recv().await {
        match event {
            XSWDEvent::CancelRequest(_, callback) => {
                let res = prompt.cancel_read_input().await;
                if callback.send(res).is_err() {
                    error!("Error while sending cancel response back to XSWD");
                }
            }
            XSWDEvent::RequestApplication(app_state, callback) => {
                let prompt = prompt.clone();
                let res = xswd_handle_request_application(&prompt, app_state).await;
                if callback.send(res).is_err() {
                    error!("Error while sending application response back to XSWD");
                }
            }
            XSWDEvent::RequestPermission(app_state, request, callback) => {
                let res = xswd_handle_request_permission(&prompt, app_state, request).await;
                if callback.send(res).is_err() {
                    error!("Error while sending permission response back to XSWD");
                }
            }
            XSWDEvent::AppDisconnect(_) => {}
        };
    }
}

#[cfg(feature = "xswd")]
async fn xswd_handle_request_application(
    prompt: &ShareablePrompt,
    app_state: AppStateShared,
) -> Result<PermissionResult, Error> {
    let mut message = format!(
        "XSWD: Application {} ({}) request access to your wallet",
        app_state.get_name(),
        app_state.get_id()
    );
    let permissions = app_state.get_permissions().lock().await;
    if !permissions.is_empty() {
        message += &format!("\r\nPermissions ({}):", permissions.len());
        for perm in permissions.keys() {
            message += &format!("\r\n- {perm}");
        }
    }

    message += "\r\n(Y/N): ";
    let accepted = prompt
        .read_valid_str_value(
            prompt.colorize_string(Color::Blue, &message),
            vec!["y", "n"],
        )
        .await?
        == "y";
    if accepted {
        Ok(PermissionResult::Accept)
    } else {
        Ok(PermissionResult::Reject)
    }
}

#[cfg(feature = "xswd")]
async fn xswd_handle_request_permission(
    prompt: &ShareablePrompt,
    app_state: AppStateShared,
    request: RpcRequest,
) -> Result<PermissionResult, Error> {
    let params = if let Some(params) = request.params {
        params.to_string()
    } else {
        "".to_string()
    };

    let message = format!(
        "XSWD: Request from {}: {}\r\nParams: {}\r\nDo you want to allow this request ?\r\n([A]llow / [D]eny / [AA] Always Allow / [AD] Always Deny): ",
        app_state.get_name(),
        request.method,
        params
    );

    let answer = prompt
        .read_valid_str_value(
            prompt.colorize_string(Color::Blue, &message),
            vec!["a", "d", "aa", "ad"],
        )
        .await?;
    Ok(match answer.as_str() {
        "a" => PermissionResult::Accept,
        "d" => PermissionResult::Reject,
        "aa" => PermissionResult::AlwaysAccept,
        "ad" => PermissionResult::AlwaysReject,
        _ => unreachable!(),
    })
}

// Apply the config passed in params
async fn apply_config(
    config: Config,
    wallet: &Arc<Wallet>,
    #[cfg(feature = "xswd")] prompt: &ShareablePrompt,
) {
    // Always connect to daemon (stateless wallet requires daemon connection)
    if log::log_enabled!(log::Level::Info) {
        info!(
            "Trying to connect to daemon at '{}'",
            config.network_handler.daemon_address
        );
    }
    if let Err(e) = wallet
        .set_online_mode(&config.network_handler.daemon_address, true)
        .await
    {
        if log::log_enabled!(log::Level::Error) {
            error!("Couldn't connect to daemon: {e:#}");
        }
        if log::log_enabled!(log::Level::Info) {
            info!("Wallet will run in stateless mode. Ensure daemon is running to use wallet features.");
        }
    } else if log::log_enabled!(log::Level::Info) {
        info!("Online mode enabled");
    }

    wallet.set_history_scan(!config.disable_history_scan);
    wallet.set_stable_balance(config.force_stable_balance);

    #[cfg(feature = "api_server")]
    {
        if config.enable_xswd && config.rpc.rpc_bind_address.is_some() {
            error!("Invalid parameters configuration: RPC Server and XSWD cannot be enabled at the same time");
            return;
        }

        if let Some(address) = config.rpc.rpc_bind_address {
            let auth_config = if let (Some(username), Some(password)) =
                (config.rpc.rpc_username, config.rpc.rpc_password)
            {
                Some(AuthConfig { username, password })
            } else {
                None
            };

            if log::log_enabled!(log::Level::Info) {
                info!(
                    "Enabling RPC Server on {} {}",
                    address,
                    if auth_config.is_some() {
                        "with authentication"
                    } else {
                        "without authentication"
                    }
                );
            }
            if let Err(e) = wallet
                .enable_rpc_server(address, auth_config, config.rpc.rpc_threads)
                .await
            {
                if log::log_enabled!(log::Level::Error) {
                    error!("Error while enabling RPC Server: {e:#}");
                }
            }
        } else if config.enable_xswd {
            match wallet.enable_xswd(config.xswd_bind_address.clone()).await {
                Ok(receiver) => {
                    if let Some(receiver) = receiver {
                        // Only clone when its necessary
                        let prompt = prompt.clone();
                        spawn_task("xswd-handler", xswd_handler(receiver, prompt));
                    }
                }
                Err(e) => {
                    if log::log_enabled!(log::Level::Error) {
                        error!("Error while enabling XSWD Server: {e}");
                    }
                }
            };
        }
    }
}

// Function to build the CommandManager when a wallet is open
async fn setup_wallet_command_manager(
    wallet: Arc<Wallet>,
    command_manager: &CommandManager,
) -> Result<(), CommandError> {
    // Ensure TOS asset is tracked for wallets created before auto-tracking was added (Issue #5 fix)
    // This allows "transfer TOS ..." to work in batch mode even on older wallets
    {
        let storage = wallet.get_storage().read().await;
        if storage.get_asset(&TOS_ASSET).await.is_err() {
            drop(storage); // Release read lock before acquiring write lock
            if log::log_enabled!(log::Level::Debug) {
                log::debug!("TOS asset not found in storage, adding it");
            }
            let mut storage = wallet.get_storage().write().await;
            storage
                .add_asset(
                    &TOS_ASSET,
                    tos_common::asset::AssetData::new(
                        tos_common::config::COIN_DECIMALS,
                        "TOS".to_string(),
                        "TOS".to_string(),
                        None, // No max supply
                        None, // No owner
                    ),
                )
                .await
                .map_err(CommandError::Any)?;
            storage
                .set_asset_name(&TOS_ASSET, "TOS".to_string())
                .await
                .map_err(CommandError::Any)?;
        }
    }

    // Delete commands for opening a wallet
    command_manager.remove_command("open")?;
    command_manager.remove_command("recover_seed")?;
    command_manager.remove_command("recover_private_key")?;
    command_manager.remove_command("create")?;

    // Add wallet commands
    command_manager.add_command(Command::with_optional_arguments(
        "change_password",
        "Set a new password to open your wallet",
        vec![
            Arg::new("old_password", ArgType::String, "Current password"),
            Arg::new("new_password", ArgType::String, "New password to set"),
        ],
        CommandHandler::Async(async_handler!(change_password)),
    ))?;
    command_manager.add_command(Command::with_arguments(
        "transfer",
        "Send asset to a specified address",
        vec![
            Arg::new("asset", ArgType::String, "Asset name or hash (e.g., TOS)"),
            Arg::new("address", ArgType::String, "Recipient wallet address"),
            Arg::new(
                "amount",
                ArgType::String,
                "Amount to transfer (in atomic units)",
            ),
        ],
        vec![Arg::new(
            "fee_type",
            ArgType::String,
            "Fee payment type: 'tos' or 'energy'",
        )],
        CommandHandler::Async(async_handler!(transfer)),
    ))?;
    command_manager.add_command(Command::with_arguments(
        "transfer_all",
        "Send all your asset balance to a specified address",
        vec![
            Arg::new("asset", ArgType::String, "Asset name or hash to transfer"),
            Arg::new("address", ArgType::String, "Recipient wallet address"),
        ],
        vec![Arg::new(
            "fee_type",
            ArgType::String,
            "Fee payment type: 'tos' or 'energy'",
        )],
        CommandHandler::Async(async_handler!(transfer_all)),
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "burn",
        "Burn amount of asset",
        vec![
            Arg::new("asset", ArgType::String, "Asset name or hash to burn"),
            Arg::new(
                "amount",
                ArgType::String,
                "Amount to burn (permanently destroyed)",
            ),
        ],
        CommandHandler::Async(async_handler!(burn)),
    ))?;
    command_manager.add_command(Command::new(
        "display_address",
        "Show your wallet address",
        CommandHandler::Async(async_handler!(display_address)),
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "balance",
        "Show the balance of requested asset; Asset must be tracked",
        vec![Arg::new(
            "asset",
            ArgType::Hash,
            "Asset hash to check balance (default: TOS)",
        )],
        CommandHandler::Async(async_handler!(balance)),
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "uno_balance",
        "Show your UNO (encrypted) balance from daemon",
        vec![Arg::new(
            "topoheight",
            ArgType::Number,
            "Optional topoheight for historical UNO balance",
        )],
        CommandHandler::Async(async_handler!(uno_balance)),
    ))?;
    command_manager.add_command(Command::with_arguments(
        "uno_transfer",
        "Send UNO (privacy) transfer to a specified address",
        vec![
            Arg::new("address", ArgType::String, "Recipient wallet address"),
            Arg::new(
                "amount",
                ArgType::String,
                "Amount to transfer (in atomic units)",
            ),
        ],
        vec![],
        CommandHandler::Async(async_handler!(uno_transfer)),
    ))?;
    command_manager.add_command(Command::with_arguments(
        "shield_transfer",
        "Shield TOS to UNO (plaintext to encrypted privacy balance)",
        vec![
            Arg::new("address", ArgType::String, "Destination wallet address"),
            Arg::new(
                "amount",
                ArgType::String,
                "Amount to shield (in atomic units)",
            ),
        ],
        vec![],
        CommandHandler::Async(async_handler!(shield_transfer)),
    ))?;
    command_manager.add_command(Command::with_arguments(
        "unshield_transfer",
        "Unshield UNO to TOS (encrypted privacy balance to plaintext)",
        vec![
            Arg::new("address", ArgType::String, "Destination wallet address"),
            Arg::new(
                "amount",
                ArgType::String,
                "Amount to unshield (in atomic units)",
            ),
        ],
        vec![],
        CommandHandler::Async(async_handler!(unshield_transfer)),
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "history",
        "Show all your transactions",
        vec![Arg::new(
            "page",
            ArgType::Number,
            "Page number for pagination (default: 0)",
        )],
        CommandHandler::Async(async_handler!(history)),
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "transaction",
        "Show a specific transaction",
        vec![Arg::new(
            "hash",
            ArgType::Hash,
            "Transaction hash to display",
        )],
        CommandHandler::Async(async_handler!(transaction)),
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "seed",
        "Show seed of selected language",
        vec![
            Arg::new(
                "language",
                ArgType::Number,
                "Language ID for seed phrase display",
            ),
            Arg::new(
                "password",
                ArgType::String,
                "Password to unlock seed phrase",
            ),
        ],
        CommandHandler::Async(async_handler!(seed)),
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "private_key",
        "Show private key in hex format (for backup/recovery)",
        vec![Arg::new(
            "password",
            ArgType::String,
            "Password to unlock private key",
        )],
        CommandHandler::Async(async_handler!(show_private_key)),
    ))?;
    command_manager.add_command(Command::new(
        "nonce",
        "Show current nonce",
        CommandHandler::Async(async_handler!(nonce)),
    ))?;
    command_manager.add_command(Command::new(
        "logout",
        "Logout from existing wallet",
        CommandHandler::Async(async_handler!(logout)),
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "export_transactions",
        "Export all your transactions in a CSV file",
        vec![Arg::new(
            "filename",
            ArgType::String,
            "Output filename for CSV export",
        )],
        CommandHandler::Async(async_handler!(export_transactions_csv)),
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "freeze_tos",
        "Freeze TOS to get energy with duration-based rewards (3/7/14 days)",
        vec![
            Arg::new("amount", ArgType::String, "Amount of TOS to freeze"),
            Arg::new(
                "duration",
                ArgType::Number,
                "Freeze duration in days (3/7/14/30, longer = higher rewards)",
            ),
        ],
        CommandHandler::Async(async_handler!(freeze_tos)),
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "freeze_tos_delegate",
        "Freeze TOS and delegate energy to other accounts",
        vec![
            Arg::new(
                "duration",
                ArgType::Number,
                "Freeze duration in days (3/7/14/30, longer = higher rewards)",
            ),
            Arg::new(
                "delegatees",
                ArgType::String,
                "Comma-separated list of delegatees as address:amount",
            ),
        ],
        CommandHandler::Async(async_handler!(freeze_tos_delegate)),
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "unfreeze_tos",
        "Unfreeze TOS (release frozen TOS after lock period)",
        vec![Arg::new(
            "amount",
            ArgType::String,
            "Amount of TOS to unfreeze",
        )],
        CommandHandler::Async(async_handler!(unfreeze_tos)),
    ))?;
    let (required_args, optional_args) = unfreeze_tos_delegate_args();
    command_manager.add_command(Command::with_arguments(
        "unfreeze_tos_delegate",
        "Unfreeze delegated TOS (delegatee optional for single-entry records)",
        required_args,
        optional_args,
        CommandHandler::Async(async_handler!(unfreeze_tos_delegate)),
    ))?;
    command_manager.add_command(Command::new(
        "withdraw_unfrozen",
        "Withdraw all expired pending unfreezes",
        CommandHandler::Async(async_handler!(withdraw_unfrozen)),
    ))?;
    command_manager.add_command(Command::new(
        "energy_info",
        "Show energy information and freeze records",
        CommandHandler::Async(async_handler!(energy_info)),
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "bind_referrer",
        "Bind a referrer to your account (one-time, immutable)",
        vec![Arg::new(
            "referrer",
            ArgType::String,
            "Referrer's wallet address",
        )],
        CommandHandler::Async(async_handler!(bind_referrer)),
    ))?;
    command_manager.add_command(Command::new(
        "referral_info",
        "Show your referral information",
        CommandHandler::Async(async_handler!(referral_info)),
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "get_uplines",
        "Get upline chain (referrer's referrer's ...)",
        vec![Arg::new(
            "levels",
            ArgType::Number,
            "Number of upline levels to query (default: 10, max: 20)",
        )],
        CommandHandler::Async(async_handler!(get_uplines)),
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "get_direct_referrals",
        "List users who have you as their referrer",
        vec![
            Arg::new("offset", ArgType::Number, "Pagination offset (default: 0)"),
            Arg::new(
                "limit",
                ArgType::Number,
                "Maximum results per page (default: 20, max: 100)",
            ),
        ],
        CommandHandler::Async(async_handler!(get_direct_referrals)),
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "set_asset_name",
        "Set the name of an asset",
        vec![
            Arg::new("asset", ArgType::Hash, "Asset hash to name"),
            Arg::new("name", ArgType::String, "Display name for the asset"),
        ],
        CommandHandler::Async(async_handler!(set_asset_name)),
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "list_assets",
        "List all detected assets",
        vec![Arg::new(
            "page",
            ArgType::Number,
            "Page number for pagination (default: 0)",
        )],
        CommandHandler::Async(async_handler!(list_assets)),
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "list_balances",
        "List all balances tracked",
        vec![Arg::new(
            "page",
            ArgType::Number,
            "Page number for pagination (default: 0)",
        )],
        CommandHandler::Async(async_handler!(list_balances)),
    ))?;
    // REMOVED: list_tracked_assets, track_asset, untrack_asset commands
    // In stateless mode, use list_balances to query account assets from daemon

    {
        command_manager.add_command(Command::new(
            "sync_status",
            "Show wallet synchronization status with daemon",
            CommandHandler::Async(async_handler!(sync_status)),
        ))?;
    }

    #[cfg(feature = "api_server")]
    {
        // Unauthenticated RPC Server can only be created by launch arguments option
        command_manager.add_command(Command::with_required_arguments(
            "start_rpc_server",
            "Start the RPC Server",
            vec![
                Arg::new(
                    "bind_address",
                    ArgType::String,
                    "Bind address for RPC server (e.g., 127.0.0.1:3000)",
                ),
                Arg::new("username", ArgType::String, "RPC authentication username"),
                Arg::new("password", ArgType::String, "RPC authentication password"),
            ],
            CommandHandler::Async(async_handler!(start_rpc_server)),
        ))?;

        command_manager.add_command(Command::new(
            "start_xswd",
            "Start the XSWD Server",
            CommandHandler::Async(async_handler!(start_xswd)),
        ))?;

        // Stop API Server (RPC or XSWD)
        command_manager.add_command(Command::new(
            "stop_api_server",
            "Stop the API (XSWD/RPC) Server",
            CommandHandler::Async(async_handler!(stop_api_server)),
        ))?;
    }

    #[cfg(feature = "xswd")]
    {
        command_manager.add_command(Command::with_optional_arguments(
            "add_xswd_relayer",
            "Add a XSWD relayer to the wallet",
            vec![Arg::new(
                "app_data",
                ArgType::String,
                "Application data for XSWD relayer",
            )],
            CommandHandler::Async(async_handler!(add_xswd_relayer)),
        ))?;
    }

    // Also add multisig commands
    command_manager.add_command(Command::with_optional_arguments(
        "multisig_setup",
        "Setup a multisig (use addresses=addr1,addr2,... for batch mode)",
        vec![
            Arg::new(
                "threshold",
                ArgType::Number,
                "Required signatures threshold",
            ),
            Arg::new(
                "addresses",
                ArgType::String,
                "Comma-separated list of participant addresses",
            ),
        ],
        CommandHandler::Async(async_handler!(multisig_setup)),
    ))?;
    command_manager.add_command(Command::with_arguments(
        "multisig_sign",
        "Sign a multisig transaction and optionally submit with collected signatures",
        vec![Arg::new(
            "tx_hash",
            ArgType::Hash,
            "Transaction hash to sign (use get_hash_for_multisig from unsigned TX)",
        )],
        vec![
            Arg::new(
                "source",
                ArgType::String,
                "Source wallet address (multisig owner) - required for participant wallets",
            ),
            Arg::new(
                "tx_data",
                ArgType::String,
                "Unsigned transaction hex data (required for submit)",
            ),
            Arg::new(
                "signatures",
                ArgType::String,
                "Other signatures as 'id:sig_hex,id:sig_hex' format",
            ),
            Arg::new(
                "submit",
                ArgType::Bool,
                "Submit transaction after adding all signatures",
            ),
        ],
        CommandHandler::Async(async_handler!(multisig_sign)),
    ))?;
    command_manager.add_command(Command::new(
        "multisig_show",
        "Show the current state of multisig",
        CommandHandler::Async(async_handler!(multisig_show)),
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "multisig_create_tx",
        "Create an unsigned transaction for multisig signing (outputs tx_hash and tx_data)",
        vec![
            Arg::new("asset", ArgType::String, "Asset to transfer (TOS or hash)"),
            Arg::new("amount", ArgType::String, "Amount to transfer"),
            Arg::new("address", ArgType::String, "Recipient address"),
        ],
        CommandHandler::Async(async_handler!(multisig_create_tx)),
    ))?;

    command_manager.add_command(Command::new(
        "tx_version",
        "See the current transaction version",
        CommandHandler::Async(async_handler!(tx_version)),
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "set_tx_version",
        "Set the transaction version",
        vec![Arg::new(
            "version",
            ArgType::Number,
            "Transaction version number",
        )],
        CommandHandler::Async(async_handler!(set_tx_version)),
    ))?;
    command_manager.add_command(Command::new(
        "status",
        "See the status of the wallet",
        CommandHandler::Async(async_handler!(status)),
    ))?;

    // Smart contract deployment command
    command_manager.add_command(Command::with_required_arguments(
        "deploy_contract",
        "Deploy a smart contract to the blockchain",
        vec![Arg::new(
            "file",
            ArgType::String,
            "Path to contract bytecode file",
        )],
        CommandHandler::Async(async_handler!(deploy_contract)),
    ))?;

    // Smart contract invocation command
    command_manager.add_command(Command::with_arguments(
        "invoke_contract",
        "Invoke a smart contract function",
        vec![
            Arg::new(
                "contract",
                ArgType::String,
                "Contract address (NOT the deployment TX hash)",
            ),
            Arg::new(
                "entry_id",
                ArgType::String,
                "Entry point function ID (0-65535)",
            ),
        ],
        vec![
            Arg::new("data", ArgType::String, "Call data in hex format"),
            Arg::new(
                "max_gas",
                ArgType::Number,
                "Maximum gas limit (default: 1000000)",
            ),
            Arg::new(
                "deposit",
                ArgType::String,
                "Amount of TOS to deposit to the contract (in atomic units)",
            ),
        ],
        CommandHandler::Async(async_handler!(invoke_contract)),
    ))?;

    // Contract info query command
    command_manager.add_command(Command::with_required_arguments(
        "get_contract_info",
        "Get information about a deployed contract",
        vec![Arg::new(
            "contract",
            ArgType::String,
            "Contract address (NOT the deployment TX hash)",
        )],
        CommandHandler::Async(async_handler!(get_contract_info)),
    ))?;

    // Get contract address from deployment TX hash
    command_manager.add_command(Command::with_required_arguments(
        "get_contract_address",
        "Get the contract address from a deployment transaction hash",
        vec![Arg::new(
            "tx_hash",
            ArgType::String,
            "Deployment transaction hash",
        )],
        CommandHandler::Async(async_handler!(get_contract_address)),
    ))?;

    // Contract balance query command
    command_manager.add_command(Command::with_required_arguments(
        "get_contract_balance",
        "Get the balance of a contract for a specific asset",
        vec![
            Arg::new(
                "contract",
                ArgType::String,
                "Contract address (deployment TX hash)",
            ),
            Arg::new(
                "asset",
                ArgType::String,
                "Asset hash (use '0' for native TOS)",
            ),
        ],
        CommandHandler::Async(async_handler!(get_contract_balance)),
    ))?;

    // Count contracts command
    command_manager.add_command(Command::new(
        "count_contracts",
        "Get the total number of deployed contracts",
        CommandHandler::Async(async_handler!(count_contracts)),
    ))?;

    // ========== TNS (TOS Name Service) Commands ==========

    command_manager.add_command(Command::with_required_arguments(
        "register_name",
        "Register a TNS name (e.g., 'alice' for alice@tos.network)",
        vec![Arg::new(
            "name",
            ArgType::String,
            "Name to register (e.g., 'alice')",
        )],
        CommandHandler::Async(async_handler!(register_name)),
    ))?;

    command_manager.add_command(Command::with_required_arguments(
        "resolve_name",
        "Resolve a TNS name to its address",
        vec![Arg::new(
            "name",
            ArgType::String,
            "Name to resolve (e.g., 'alice' or 'alice@tos.network')",
        )],
        CommandHandler::Async(async_handler!(resolve_name)),
    ))?;

    command_manager.add_command(Command::with_arguments(
        "send_message",
        "Send an ephemeral message to a TNS name",
        vec![
            Arg::new(
                "recipient",
                ArgType::String,
                "Recipient name (e.g., 'bob' or 'bob@tos.network')",
            ),
            Arg::new(
                "message",
                ArgType::String,
                "Message content (max 140 bytes)",
            ),
        ],
        vec![Arg::new(
            "ttl",
            ArgType::Number,
            "Time-to-live in blocks (100-86400, default: 100)",
        )],
        CommandHandler::Async(async_handler!(send_message)),
    ))?;

    command_manager.add_command(Command::with_optional_arguments(
        "list_messages",
        "List ephemeral messages received by your registered name",
        vec![Arg::new(
            "page",
            ArgType::Number,
            "Page number for pagination (default: 0)",
        )],
        CommandHandler::Async(async_handler!(list_messages)),
    ))?;

    command_manager.add_command(Command::with_required_arguments(
        "read_message",
        "Read a specific ephemeral message by ID",
        vec![Arg::new("message_id", ArgType::Hash, "Message ID to read")],
        CommandHandler::Async(async_handler!(read_message)),
    ))?;

    let mut context = command_manager.get_context().lock()?;
    context.store(wallet);

    Ok(())
}

// Open a wallet based on the wallet name and its password
async fn open_wallet(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("open", &args)?;

    let prompt = manager.get_prompt();
    let config: Config = Config::parse();

    // Priority: command line args -> config file -> error (batch mode only)
    let dir = if args.has_argument("name") {
        let name = args.get_value("name")?.to_string_value()?;
        format!("{}{}", DIR_PATH, name)
    } else if let Some(path) = config.wallet_path.as_ref() {
        path.clone()
    } else {
        return Err(CommandError::MissingArgument("name".to_string()));
    };

    if !Path::new(&dir).is_dir() {
        manager.message("No wallet found with this name");
        return Ok(());
    }

    let password = if args.has_argument("password") {
        args.get_value("password")?.to_string_value()?
    } else if let Some(pwd) = config.password.as_ref() {
        pwd.clone()
    } else {
        return Err(CommandError::MissingArgument("password".to_string()));
    };

    let wallet = {
        let context = manager.get_context().lock()?;
        let network = context.get::<Network>()?;
        let precomputed_tables = precomputed_tables::read_or_generate_precomputed_tables(
            config.precomputed_tables.precomputed_tables_path.as_deref(),
            config.precomputed_tables.precomputed_tables_l1,
            LogProgressTableGenerationReportFunction,
            true,
        )
        .await?;
        Wallet::open(
            &dir,
            &password,
            *network,
            precomputed_tables,
            config.n_decryption_threads,
            config.network_concurrency,
        )?
    };

    manager.message("Wallet sucessfully opened");
    apply_config(
        config,
        &wallet,
        #[cfg(feature = "xswd")]
        prompt,
    )
    .await;

    setup_wallet_command_manager(wallet, manager).await?;

    Ok(())
}

// Create a wallet by requesting name, password
async fn create_wallet(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("create", &args)?;

    let prompt = manager.get_prompt();
    let config: Config = Config::parse();

    // Priority: command line args -> config file -> error (batch mode only)
    let dir = if args.has_argument("name") {
        let name = args.get_value("name")?.to_string_value()?;
        format!("{}{}", DIR_PATH, name)
    } else if let Some(path) = config.wallet_path.as_ref() {
        path.clone()
    } else {
        return Err(CommandError::MissingArgument("name".to_string()));
    };

    if Path::new(&dir).is_dir() {
        manager.message("wallet already exists with this name");
        return Ok(());
    }

    // Handle password input (batch mode only)
    let password = if args.has_argument("password") {
        args.get_value("password")?.to_string_value()?
    } else if let Some(pwd) = config.password.as_ref() {
        pwd.clone()
    } else {
        return Err(CommandError::MissingArgument("password".to_string()));
    };

    let wallet = {
        let context = manager.get_context().lock()?;
        let network = context.get::<Network>()?;
        let precomputed_tables = precomputed_tables::read_or_generate_precomputed_tables(
            config.precomputed_tables.precomputed_tables_path.as_deref(),
            precomputed_tables::L1_FULL,
            LogProgressTableGenerationReportFunction,
            true,
        )
        .await?;
        Wallet::create(
            &dir,
            &password,
            None,
            *network,
            precomputed_tables,
            config.n_decryption_threads,
            config.network_concurrency,
        )
        .await?
    };

    manager.message("Wallet sucessfully created");
    apply_config(
        config,
        &wallet,
        #[cfg(feature = "xswd")]
        prompt,
    )
    .await;

    // Display the seed (batch mode only)
    {
        let seed = wallet.get_seed(0)?; // 0 = English (default language)
        manager.message(format!("Seed: {}", seed));
        manager.message("IMPORTANT: Please save this seed phrase in a secure location.");
    }

    setup_wallet_command_manager(wallet, manager).await?;

    Ok(())
}

// Recover a wallet by requesting its seed or private key, name and password
async fn recover_wallet(
    manager: &CommandManager,
    mut args: ArgumentManager,
    seed: bool,
) -> Result<(), CommandError> {
    #[cfg(feature = "xswd")]
    let prompt = manager.get_prompt();
    let config: Config = Config::parse();
    // Priority: command line args -> config file -> error (batch mode only)
    let dir = if args.has_argument("name") {
        let name = args.get_value("name")?.to_string_value()?;
        format!("{}{}", DIR_PATH, name)
    } else if let Some(path) = config.wallet_path.as_ref() {
        path.clone()
    } else {
        return Err(CommandError::MissingArgument("name".to_string()));
    };

    if Path::new(&dir).is_dir() {
        manager.message("Wallet already exists with this name");
        return Ok(());
    }

    let content = if seed {
        let seed = if args.has_argument("seed") {
            args.get_value("seed")?.to_string_value()?
        } else if let Some(s) = config.seed.as_ref() {
            s.clone()
        } else {
            return Err(CommandError::MissingArgument("seed".to_string()));
        };

        let words_count = seed.split_whitespace().count();
        if words_count != 25 && words_count != 24 {
            manager.error("Seed must be 24 or 25 (checksum) words long");
            return Ok(());
        }
        seed
    } else {
        let private_key = if args.has_argument("private_key") {
            args.get_value("private_key")?.to_string_value()?
        } else {
            return Err(CommandError::MissingArgument("private_key".to_string()));
        };

        if private_key.len() != 64 {
            manager.error("Private key must be 64 characters long");
            return Ok(());
        }
        private_key
    };

    // Handle password input (batch mode only)
    let password = if args.has_argument("password") {
        args.get_value("password")?.to_string_value()?
    } else if let Some(pwd) = config.password.as_ref() {
        pwd.clone()
    } else {
        return Err(CommandError::MissingArgument("password".to_string()));
    };

    let wallet = {
        let context = manager.get_context().lock()?;
        let network = context.get::<Network>()?;
        let precomputed_tables = precomputed_tables::read_or_generate_precomputed_tables(
            config.precomputed_tables.precomputed_tables_path.as_deref(),
            config.precomputed_tables.precomputed_tables_l1,
            LogProgressTableGenerationReportFunction,
            true,
        )
        .await?;

        let recover = if seed {
            RecoverOption::Seed(&content)
        } else {
            RecoverOption::PrivateKey(&content)
        };
        Wallet::create(
            &dir,
            &password,
            Some(recover),
            *network,
            precomputed_tables,
            config.n_decryption_threads,
            config.network_concurrency,
        )
        .await?
    };

    manager.message("Wallet sucessfully recovered");
    apply_config(
        config,
        &wallet,
        #[cfg(feature = "xswd")]
        prompt,
    )
    .await;

    setup_wallet_command_manager(wallet, manager).await?;

    Ok(())
}

async fn recover_seed(manager: &CommandManager, args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("recover_seed", &args)?;
    recover_wallet(manager, args, true).await
}

async fn recover_private_key(
    manager: &CommandManager,
    args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("recover_private_key", &args)?;
    recover_wallet(manager, args, false).await
}

// Set the asset name (not supported in stateless wallet)
async fn set_asset_name(
    manager: &CommandManager,
    _args: ArgumentManager,
) -> Result<(), CommandError> {
    // Stateless wallet: asset names are not stored locally
    manager.message("Note: Asset name setting is not supported in stateless wallet mode.");
    manager.message("Asset data is fetched from daemon on-demand.");
    Ok(())
}

// List assets (not supported in stateless wallet)
async fn list_assets(manager: &CommandManager, _args: ArgumentManager) -> Result<(), CommandError> {
    // Stateless wallet: assets are not tracked locally
    manager.message("Note: Asset listing is not supported in stateless wallet mode.");
    manager.message("Use 'list_balances' to see your current balances from the daemon.");
    Ok(())
}

// NOTE: This command now queries 100% from daemon API (no local storage)
async fn list_balances(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let page = if args.has_argument("page") {
        args.get_value("page")?.to_number()? as usize
    } else {
        0
    };

    // Query all data from daemon API (stateless - no local storage)

    {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();
        let address = wallet.get_address();

        // Get account assets from daemon
        let assets = daemon_api
            .get_account_assets(&address, None, None)
            .await
            .map_err(|e| {
                CommandError::InvalidArgument(format!(
                    "Failed to get account assets from daemon: {}",
                    e
                ))
            })?;

        let assets_vec: Vec<_> = assets.into_iter().collect();
        let count = assets_vec.len();

        if count == 0 {
            manager.message("No balances found");
            return Ok(());
        }

        let mut max_pages = count / ELEMENTS_PER_PAGE;
        if count % ELEMENTS_PER_PAGE != 0 {
            max_pages += 1;
        }

        if page > max_pages {
            return Err(CommandError::InvalidArgument(format!(
                "Page must be less than maximum pages ({})",
                max_pages - 1
            )));
        }

        manager.message(format!("Balances (page {}/{}):", page, max_pages));

        for asset in assets_vec
            .iter()
            .skip(page * ELEMENTS_PER_PAGE)
            .take(ELEMENTS_PER_PAGE)
        {
            // Query balance and asset info from daemon
            let balance = daemon_api
                .get_balance(&address, asset)
                .await
                .map(|r| r.balance)
                .unwrap_or(0);
            let data = daemon_api.get_asset(asset).await.ok();

            if let Some(data) = data {
                manager.message(format!(
                    "Balance for asset {} ({}): {}",
                    data.inner.get_name(),
                    asset,
                    format_coin(balance, data.inner.get_decimals())
                ));
            } else {
                manager.message(format!(
                    "Balance for asset {}: {} (no asset data)",
                    asset,
                    format_tos(balance)
                ));
            }
        }
    }

    Ok(())
}

// REMOVED: list_tracked_assets, track_asset, untrack_asset functions
// In stateless mode, use list_balances to query account assets from daemon

// Change wallet password
async fn change_password(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("change_password", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let old_password = if args.has_argument("old_password") {
        args.get_value("old_password")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument("old_password".to_string()));
    };

    let new_password = if args.has_argument("new_password") {
        args.get_value("new_password")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument("new_password".to_string()));
    };

    manager.message("Changing password...");
    wallet.set_password(&old_password, &new_password).await?;
    manager.message("Your password has been changed!");
    Ok(())
}

async fn create_transaction_with_multisig(
    manager: &CommandManager,
    prompt: &Prompt,
    wallet: &Wallet,
    tx_type: TransactionTypeBuilder,
    payload: MultiSigPayload,
) -> Result<Transaction, CommandError> {
    manager.message(format!(
        "Multisig detected, you need to sign the transaction with {} keys.",
        payload.threshold
    ));

    let mut storage = wallet.get_storage().write().await;
    let fee = FeeBuilder::default();
    let mut state = wallet
        .create_transaction_state_with_storage(&storage, &tx_type, &fee, None)
        .await
        .context("Error while creating transaction state")?;

    let mut unsigned = wallet
        .create_unsigned_transaction(
            &mut state,
            Some(payload.threshold),
            tx_type,
            fee,
            storage.get_tx_version().await?,
        )
        .context("Error while building unsigned transaction")?;

    let mut multisig = MultiSig::new();
    manager.message(format!(
        "Transaction hash to sign: {}",
        unsigned.get_hash_for_multisig()
    ));

    if payload.threshold == 1 {
        let signature = prompt
            .read_input("Enter signature hexadecimal: ", false)
            .await
            .context("Error while reading signature")?;
        let signature = Signature::from_hex(&signature).context("Invalid signature")?;

        let id = if payload.participants.len() == 1 {
            0
        } else {
            prompt
                .read("Enter signer ID: ")
                .await
                .context("Error while reading signer id")?
        };

        if !multisig.add_signature(SignatureId { id, signature }) {
            return Err(CommandError::InvalidArgument(
                "Invalid signature".to_string(),
            ));
        }
    } else {
        manager.message("Participants available:");
        for (i, participant) in payload.participants.iter().enumerate() {
            manager.message(format!(
                "Participant #{}: {}",
                i,
                participant.as_address(wallet.get_network().is_mainnet())
            ));
        }

        manager.message("Please enter the signatures and signer IDs");
        for i in 0..payload.threshold {
            let signature = prompt
                .read_input(format!("Enter signature #{} hexadecimal: ", i), false)
                .await
                .context("Error while reading signature")?;
            let signature = Signature::from_hex(&signature).context("Invalid signature")?;

            let id = prompt
                .read("Enter signer ID for signature: ")
                .await
                .context("Error while reading signer id")?;

            if !multisig.add_signature(SignatureId { id, signature }) {
                return Err(CommandError::InvalidArgument(
                    "Invalid signature".to_string(),
                ));
            }
        }
    }

    unsigned.set_multisig(multisig);

    let tx = unsigned.finalize(wallet.get_keypair());
    state.set_tx_hash_built(tx.hash());

    state
        .apply_changes(&mut storage)
        .await
        .context("Error while applying changes")?;

    Ok(tx)
}

// Create a new transfer to a specified address
// NOTE: This command now queries balance/asset from daemon API (no local storage)
async fn transfer(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("transfer", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // read address (batch mode only) - supports both regular addresses and TNS names
    let str_address = if args.has_argument("address") {
        args.get_value("address")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument("address".to_string()));
    };

    // Check if this is a TNS name (ends with @tos.network)
    let address = if str_address.ends_with("@tos.network") {
        // Extract the name part (without @tos.network suffix)
        let name_part = &str_address[..str_address.len() - 12];

        // Resolve TNS name to address
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();

        let result = daemon_api.resolve_name(name_part).await.map_err(|e| {
            CommandError::Any(anyhow::anyhow!(
                "Failed to resolve TNS name '{}': {}",
                name_part,
                e
            ))
        })?;

        match result.address {
            Some(addr) => {
                manager.message(format!("Resolved {}@tos.network -> {}", name_part, addr));
                addr.into_owned()
            }
            None => {
                return Err(CommandError::InvalidArgument(format!(
                    "TNS name '{}' is not registered",
                    name_part
                )));
            }
        }
    } else {
        Address::from_string(&str_address).context("Invalid address")?
    };

    // Parse asset - TOS or hash only (no name lookup from local storage)
    let asset = if args.has_argument("asset") {
        let asset_str = args.get_value("asset")?.to_string_value()?;
        if asset_str.is_empty() || asset_str.trim().is_empty() || asset_str.to_uppercase() == "TOS"
        {
            TOS_ASSET
        } else if asset_str.len() == HASH_SIZE * 2 {
            Hash::from_hex(&asset_str).context("Error while parsing asset hash from hex")?
        } else {
            return Err(CommandError::InvalidArgument(format!(
                "Invalid asset '{}'. Use asset hash (64 hex chars) or 'TOS' for native token.",
                asset_str
            )));
        }
    } else {
        return Err(CommandError::MissingArgument("asset".to_string()));
    };

    // Query balance, asset data, and multisig from daemon API (stateless)

    let (_max_balance, asset_data, multisig) = {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();
        let wallet_address = wallet.get_address();

        // Get balance from daemon
        let balance = daemon_api
            .get_balance(&wallet_address, &asset)
            .await
            .map(|r| r.balance)
            .unwrap_or(0);

        // Get asset data from daemon
        let asset_data = daemon_api.get_asset(&asset).await.map_err(|e| {
            CommandError::InvalidArgument(format!("Failed to get asset info from daemon: {}", e))
        })?;

        // Get multisig state from daemon
        let multisig = if daemon_api
            .has_multisig(&wallet_address)
            .await
            .unwrap_or(false)
        {
            daemon_api.get_multisig(&wallet_address).await.ok()
        } else {
            None
        };

        (balance, asset_data, multisig)
    };

    // read amount (batch mode only)
    let amount = if args.has_argument("amount") {
        args.get_value("amount")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument("amount".to_string()));
    };

    let amount = from_coin(amount, asset_data.inner.get_decimals()).context("Invalid amount")?;

    // Read fee_type parameter
    let fee_type = if args.has_argument("fee_type") {
        let fee_type_str = args.get_value("fee_type")?.to_string_value()?;
        match fee_type_str.to_lowercase().as_str() {
            "tos" => Some(tos_common::transaction::FeeType::TOS),
            "energy" => Some(tos_common::transaction::FeeType::Energy),
            "uno" => Some(tos_common::transaction::FeeType::UNO),
            _ => {
                manager.error("Invalid fee_type. Use 'tos', 'energy', or 'uno'");
                return Ok(());
            }
        }
    } else {
        None
    };

    // Validate fee_type for energy
    if fee_type.as_ref() == Some(&tos_common::transaction::FeeType::Energy) && asset != TOS_ASSET {
        manager.error("Energy fees can only be used for TOS transfers");
        return Ok(());
    }

    manager.message(format!(
        "Sending {} of {} ({}) to {}",
        format_coin(amount, asset_data.inner.get_decimals()),
        asset_data.inner.get_name(),
        asset,
        address
    ));

    manager.message("Building transaction...");
    let transfer = TransferBuilder {
        destination: address,
        amount,
        asset,
        extra_data: None,
    };
    let tx_type = TransactionTypeBuilder::Transfers(vec![transfer]);

    // Get multisig threshold from daemon result if active
    let multisig_threshold = multisig.and_then(|m| {
        use tos_common::api::daemon::MultisigState;
        match m.state {
            MultisigState::Active { threshold, .. } => Some(threshold),
            MultisigState::Deleted => None,
        }
    });

    if let Some(threshold) = multisig_threshold {
        manager.message(format!(
            "Multisig detected (threshold: {}). Note: Full multisig signing not supported in stateless mode.",
            threshold
        ));
    }

    // Create transaction state and builder using existing wallet infrastructure
    // (which already has daemon fallback for nonce/balance queries)
    let storage = wallet.get_storage().read().await;
    let mut state = wallet
        .create_transaction_state_with_storage(&storage, &tx_type, &FeeBuilder::default(), None)
        .await
        .context("Error while creating transaction state")?;

    // Create transaction with fee type
    let tx_version = storage
        .get_tx_version()
        .await
        .context("Error while getting tx version")?;

    // Create a custom fee builder if energy fees are requested
    let fee_builder = if let Some(ref ft) = fee_type {
        if *ft == tos_common::transaction::FeeType::Energy {
            FeeBuilder::Value(0) // Energy fees are 0 TOS
        } else {
            FeeBuilder::default()
        }
    } else {
        FeeBuilder::default()
    };

    // Create transaction builder with fee type
    let mut builder = tos_common::transaction::builder::TransactionBuilder::new(
        tx_version,
        wallet.get_network().chain_id() as u8,
        wallet.get_public_key().clone(),
        multisig_threshold,
        tx_type,
        fee_builder,
    );

    // Set fee type if specified
    if let Some(ref ft) = fee_type {
        builder = builder.with_fee_type(ft.clone());
    }

    let tx = match builder.build(&mut state, wallet.get_keypair()) {
        Ok(tx) => {
            manager.message(format!(
                "Transaction created with {} fees",
                match fee_type
                    .as_ref()
                    .unwrap_or(&tos_common::transaction::FeeType::TOS)
                {
                    tos_common::transaction::FeeType::TOS => "TOS",
                    tos_common::transaction::FeeType::Energy => "Energy",
                    tos_common::transaction::FeeType::UNO => "UNO",
                }
            ));
            tx
        }
        Err(e) => {
            manager.error(format!("Error while creating transaction: {}", e));
            return Ok(());
        }
    };

    broadcast_tx(wallet, manager, tx).await;
    Ok(())
}

// Send the whole balance to a specified address
async fn transfer_all(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("transfer_all", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // read address (batch mode only) - supports both regular addresses and TNS names
    let str_address = if args.has_argument("address") {
        args.get_value("address")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument("address".to_string()));
    };

    // Check if this is a TNS name (ends with @tos.network)
    let address = if str_address.ends_with("@tos.network") {
        // Extract the name part (without @tos.network suffix)
        let name_part = &str_address[..str_address.len() - 12];

        // Resolve TNS name to address
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();

        let result = daemon_api.resolve_name(name_part).await.map_err(|e| {
            CommandError::Any(anyhow::anyhow!(
                "Failed to resolve TNS name '{}': {}",
                name_part,
                e
            ))
        })?;

        match result.address {
            Some(addr) => {
                manager.message(format!("Resolved {}@tos.network -> {}", name_part, addr));
                addr.into_owned()
            }
            None => {
                return Err(CommandError::InvalidArgument(format!(
                    "TNS name '{}' is not registered",
                    name_part
                )));
            }
        }
    } else {
        Address::from_string(&str_address).context("Invalid address")?
    };

    // Parse asset (batch mode only)
    let asset = if args.has_argument("asset") {
        let asset_str = args.get_value("asset")?.to_string_value()?;
        if asset_str.is_empty() || asset_str.to_uppercase() == "TOS" {
            TOS_ASSET
        } else if asset_str.len() == HASH_SIZE * 2 {
            Hash::from_hex(&asset_str).context("Error while parsing asset hash from hex")?
        } else {
            return Err(CommandError::InvalidArgument(format!(
                "Invalid asset '{}'. Use asset hash (64 hex chars) or 'TOS' for native token.",
                asset_str
            )));
        }
    } else {
        return Err(CommandError::MissingArgument("asset".to_string()));
    };

    // Read fee_type parameter
    let fee_type = if args.has_argument("fee_type") {
        let fee_type_str = args.get_value("fee_type")?.to_string_value()?;
        match fee_type_str.to_lowercase().as_str() {
            "tos" => Some(tos_common::transaction::FeeType::TOS),
            "energy" => Some(tos_common::transaction::FeeType::Energy),
            "uno" => Some(tos_common::transaction::FeeType::UNO),
            _ => {
                manager.error("Invalid fee_type. Use 'tos', 'energy', or 'uno'");
                return Ok(());
            }
        }
    } else {
        None
    };

    // Validate fee_type for energy
    if fee_type.as_ref() == Some(&tos_common::transaction::FeeType::Energy) && asset != TOS_ASSET {
        manager.error("Energy fees can only be used for TOS transfers");
        return Ok(());
    }

    // Query balance, asset data, and multisig from daemon API (stateless)

    let (mut amount, asset_data, multisig) = {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();
        let wallet_address = wallet.get_address();

        // Get balance from daemon
        let balance = daemon_api
            .get_balance(&wallet_address, &asset)
            .await
            .map(|r| r.balance)
            .unwrap_or(0);

        // Get asset data from daemon
        let asset_data = daemon_api.get_asset(&asset).await.map_err(|e| {
            CommandError::InvalidArgument(format!("Failed to get asset info from daemon: {}", e))
        })?;

        // Get multisig state from daemon
        let multisig = if daemon_api
            .has_multisig(&wallet_address)
            .await
            .unwrap_or(false)
        {
            daemon_api.get_multisig(&wallet_address).await.ok()
        } else {
            None
        };

        (balance, asset_data, multisig)
    };

    let transfer = TransferBuilder {
        destination: address.clone(),
        amount,
        asset: asset.clone(),
        extra_data: None,
    };
    let tx_type = TransactionTypeBuilder::Transfers(vec![transfer]);

    // Estimate fees based on fee type
    let estimated_fees = if let Some(ref ft) = fee_type {
        if *ft == tos_common::transaction::FeeType::Energy {
            // For energy fees, we don't deduct from TOS balance
            0
        } else {
            wallet
                .estimate_fees(tx_type.clone(), FeeBuilder::default())
                .await
                .context("Error while estimating fees")?
        }
    } else {
        wallet
            .estimate_fees(tx_type.clone(), FeeBuilder::default())
            .await
            .context("Error while estimating fees")?
    };

    if asset == TOS_ASSET && fee_type.as_ref() != Some(&tos_common::transaction::FeeType::Energy) {
        amount = amount
            .checked_sub(estimated_fees)
            .context("Insufficient balance to pay fees")?;
    }

    let fee_display = if let Some(ref ft) = fee_type {
        match ft {
            tos_common::transaction::FeeType::TOS => {
                format!("TOS fees: {}", format_tos(estimated_fees))
            }
            tos_common::transaction::FeeType::Energy => "Energy fees: 0 TOS".to_string(),
            tos_common::transaction::FeeType::UNO => "UNO fees: 0 TOS".to_string(),
        }
    } else {
        format!("TOS fees: {}", format_tos(estimated_fees))
    };

    manager.message(format!(
        "Sending {} of {} ({}) to {} ({})",
        format_coin(amount, asset_data.inner.get_decimals()),
        asset_data.inner.get_name(),
        asset,
        address,
        fee_display
    ));

    manager.message("Building transaction...");
    let transfer = TransferBuilder {
        destination: address,
        amount,
        asset,
        extra_data: None,
    };
    let tx_type = TransactionTypeBuilder::Transfers(vec![transfer]);

    // Check multisig and show warning (stateless mode doesn't support full multisig signing)
    let multisig_threshold = multisig.and_then(|m| {
        use tos_common::api::daemon::MultisigState;
        match m.state {
            MultisigState::Active { threshold, .. } => Some(threshold),
            MultisigState::Deleted => None,
        }
    });

    if let Some(threshold) = multisig_threshold {
        manager.message(format!(
            "Multisig detected (threshold: {}). Note: Full multisig signing not supported in stateless mode.",
            threshold
        ));
    }

    let tx = {
        // Create transaction with appropriate fee type
        let storage = wallet.get_storage().read().await;
        let mut state = wallet
            .create_transaction_state_with_storage(&storage, &tx_type, &FeeBuilder::default(), None)
            .await
            .context("Error while creating transaction state")?;

        // Create transaction with fee type
        let tx_version = storage
            .get_tx_version()
            .await
            .context("Error while getting tx version")?;
        let threshold = None;

        // Create a custom fee builder if energy fees are requested
        let fee_builder = if let Some(ref ft) = fee_type {
            if *ft == tos_common::transaction::FeeType::Energy {
                FeeBuilder::Value(0) // Energy fees are 0 TOS
            } else {
                FeeBuilder::default()
            }
        } else {
            FeeBuilder::default()
        };

        // Create transaction builder with fee type
        let mut builder = tos_common::transaction::builder::TransactionBuilder::new(
            tx_version,
            wallet.get_network().chain_id() as u8,
            wallet.get_public_key().clone(),
            threshold,
            tx_type,
            fee_builder,
        );

        // Set fee type if specified
        if let Some(ref ft) = fee_type {
            builder = builder.with_fee_type(ft.clone());
        }

        match builder.build(&mut state, wallet.get_keypair()) {
            Ok(tx) => {
                manager.message(format!(
                    "Transaction created with {} fees",
                    match fee_type
                        .as_ref()
                        .unwrap_or(&tos_common::transaction::FeeType::TOS)
                    {
                        tos_common::transaction::FeeType::TOS => "TOS",
                        tos_common::transaction::FeeType::Energy => "Energy",
                        tos_common::transaction::FeeType::UNO => "UNO",
                    }
                ));
                tx
            }
            Err(e) => {
                manager.error(format!("Error while creating transaction: {}", e));
                return Ok(());
            }
        }
    };

    broadcast_tx(wallet, manager, tx).await;
    Ok(())
}

async fn burn(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("burn", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Parse asset (batch mode only)
    let asset = if args.has_argument("asset") {
        let asset_str = args.get_value("asset")?.to_string_value()?;
        if asset_str.is_empty() || asset_str.to_uppercase() == "TOS" {
            TOS_ASSET
        } else if asset_str.len() == HASH_SIZE * 2 {
            Hash::from_hex(&asset_str).context("Error while parsing asset hash from hex")?
        } else {
            return Err(CommandError::InvalidArgument(format!(
                "Invalid asset '{}'. Use asset hash (64 hex chars) or 'TOS' for native token.",
                asset_str
            )));
        }
    } else {
        return Err(CommandError::MissingArgument("asset".to_string()));
    };

    // Query balance, asset data, and multisig from daemon API (stateless)

    let (asset_data, multisig) = {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();
        let wallet_address = wallet.get_address();

        // Get asset data from daemon
        let asset_data = daemon_api.get_asset(&asset).await.map_err(|e| {
            CommandError::InvalidArgument(format!("Failed to get asset info from daemon: {}", e))
        })?;

        // Get multisig state from daemon
        let multisig = if daemon_api
            .has_multisig(&wallet_address)
            .await
            .unwrap_or(false)
        {
            daemon_api.get_multisig(&wallet_address).await.ok()
        } else {
            None
        };

        (asset_data, multisig)
    };

    // read amount (batch mode only)
    let amount = if args.has_argument("amount") {
        args.get_value("amount")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument("amount".to_string()));
    };

    let amount = from_coin(amount, asset_data.inner.get_decimals()).context("Invalid amount")?;
    manager.message(format!(
        "Burning {} of {} ({})",
        format_coin(amount, asset_data.inner.get_decimals()),
        asset_data.inner.get_name(),
        asset
    ));

    manager.message("Building transaction...");
    let payload = BurnPayload { amount, asset };

    let tx_type = TransactionTypeBuilder::Burn(payload);

    // Check multisig and show warning (stateless mode doesn't support full multisig signing)
    let multisig_threshold = multisig.and_then(|m| {
        use tos_common::api::daemon::MultisigState;
        match m.state {
            MultisigState::Active { threshold, .. } => Some(threshold),
            MultisigState::Deleted => None,
        }
    });

    if let Some(threshold) = multisig_threshold {
        manager.message(format!(
            "Multisig detected (threshold: {}). Note: Full multisig signing not supported in stateless mode.",
            threshold
        ));
    }

    let tx = match wallet
        .create_transaction(tx_type, FeeBuilder::default())
        .await
    {
        Ok(tx) => tx,
        Err(e) => {
            manager.error(format!("Error while creating transaction: {}", e));
            return Ok(());
        }
    };

    broadcast_tx(wallet, manager, tx).await;
    Ok(())
}

// Show current wallet address
async fn display_address(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    manager.message(format!("Wallet address: {}", wallet.get_address()));
    Ok(())
}

// Show current balance for specified asset or list all non-zero balances
// NOTE: This command now queries 100% from daemon API (no local storage)
async fn balance(
    manager: &CommandManager,
    mut arguments: ArgumentManager,
) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Parse asset (default to TOS if not specified)
    let asset = if arguments.has_argument("asset") {
        arguments.get_value("asset")?.to_hash()?
    } else {
        TOS_ASSET // Default to TOS
    };

    // Query balance from daemon API (stateless - no local storage)

    let balance = {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let address = wallet.get_address();
        let result = handler
            .get_api()
            .get_balance(&address, &asset)
            .await
            .map_err(|e| {
                CommandError::InvalidArgument(format!("Failed to get balance from daemon: {}", e))
            })?;
        result.balance
    };

    // Query asset info from daemon

    let data = {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        handler.get_api().get_asset(&asset).await.map_err(|e| {
            CommandError::InvalidArgument(format!("Failed to get asset info from daemon: {}", e))
        })?
    };

    manager.message(format!(
        "Balance for asset {} ({}): {}",
        data.inner.get_name(),
        asset,
        format_coin(balance, data.inner.get_decimals())
    ));
    Ok(())
}

// Show UNO (encrypted) balance from daemon
async fn uno_balance(
    manager: &CommandManager,
    mut arguments: ArgumentManager,
) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Query UNO balance from daemon API
    let network_handler = wallet.get_network_handler().lock().await;
    let handler = network_handler.as_ref().ok_or_else(|| {
        CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
    })?;
    let address = wallet.get_address();

    // Check if account has UNO balance
    let has_uno = handler
        .get_api()
        .has_uno_balance(&address)
        .await
        .map_err(|e| {
            CommandError::InvalidArgument(format!("Failed to check UNO balance: {}", e))
        })?;

    if !has_uno {
        manager.message("No UNO balance found for this address.");
        manager.message("(Shield command to convert TOS to UNO coming soon)");
        return Ok(());
    }

    // Get the UNO balance (optionally at a topoheight)
    let result = if arguments.has_argument("topoheight") {
        let topoheight = arguments.get_value("topoheight")?.to_number()?;
        let version = handler
            .get_api()
            .get_uno_balance_at_topoheight(&address, topoheight)
            .await
            .map_err(|e| {
                CommandError::InvalidArgument(format!(
                    "Failed to get UNO balance at topoheight: {}",
                    e
                ))
            })?;
        tos_common::api::daemon::GetUnoBalanceResult {
            version,
            topoheight,
        }
    } else {
        handler
            .get_api()
            .get_uno_balance(&address)
            .await
            .map_err(|e| {
                CommandError::InvalidArgument(format!("Failed to get UNO balance: {}", e))
            })?
    };

    manager.message("UNO (Encrypted) Balance:");
    manager.message(format!("  Topoheight: {}", result.topoheight));
    manager.message(format!(
        "  Balance Type: {:?}",
        result.version.get_balance_type()
    ));
    manager.message("  (Encrypted balance - decrypt with your private key to see amount)");

    Ok(())
}

// Send UNO (privacy) transfer to a specified address
async fn uno_transfer(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    use tos_common::account::CiphertextCache;
    use tos_common::config::UNO_ASSET;
    use tos_wallet::transaction_builder::{TransactionBuilderState, UnoBalance};

    manager.validate_batch_params("uno_transfer", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Parse address
    let str_address = args.get_value("address")?.to_string_value()?;
    let address = Address::from_string(&str_address).context("Invalid address")?;

    // Parse amount
    let amount_str = args.get_value("amount")?.to_string_value()?;
    // UNO uses same decimals as TOS (8 decimals)
    let amount =
        from_coin(amount_str, tos_common::config::COIN_DECIMALS).context("Invalid amount")?;

    if amount == 0 {
        manager.error("Amount must be greater than 0");
        return Ok(());
    }

    // Get UNO balance from daemon
    let network_handler = wallet.get_network_handler().lock().await;
    let handler = network_handler.as_ref().ok_or_else(|| {
        CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
    })?;
    let wallet_address = wallet.get_address();

    // Check if account has UNO balance
    let has_uno = handler
        .get_api()
        .has_uno_balance(&wallet_address)
        .await
        .map_err(|e| {
            CommandError::InvalidArgument(format!("Failed to check UNO balance: {}", e))
        })?;

    if !has_uno {
        manager.error("No UNO balance found for this address.");
        manager.message("(Shield command to convert TOS to UNO coming soon)");
        return Ok(());
    }

    // Get the UNO balance (ciphertext)
    let uno_result = handler
        .get_api()
        .get_uno_balance(&wallet_address)
        .await
        .map_err(|e| CommandError::InvalidArgument(format!("Failed to get UNO balance: {}", e)))?;

    // Decrypt the balance using wallet's keypair
    manager.message("Decrypting UNO balance...");
    let mut versioned_balance = uno_result.version;
    let ciphertext = versioned_balance
        .get_mut_balance()
        .decompressed()
        .map_err(|e| {
            CommandError::InvalidArgument(format!("Failed to decompress ciphertext: {}", e))
        })?
        .clone();

    // Decrypt using ECDLP tables
    let precomputed_tables = wallet.get_precomputed_tables();
    let tables_guard = precomputed_tables.read().map_err(|e| {
        CommandError::InvalidArgument(format!("Failed to acquire ECDLP tables lock: {}", e))
    })?;
    let tables_view = tables_guard.view();
    let decrypted_amount = wallet
        .get_keypair()
        .decrypt(&tables_view, &ciphertext)
        .ok_or_else(|| {
            CommandError::InvalidArgument(
                "Failed to decrypt UNO balance (value too large or corrupted)".to_string(),
            )
        })?;
    drop(tables_guard);

    manager.message(format!(
        "Current UNO balance: {} UNO",
        format_coin(decrypted_amount, tos_common::config::COIN_DECIMALS)
    ));

    // Check sufficient balance
    if decrypted_amount < amount {
        manager.error(format!(
            "Insufficient UNO balance. Have: {}, Need: {}",
            format_coin(decrypted_amount, tos_common::config::COIN_DECIMALS),
            format_coin(amount, tos_common::config::COIN_DECIMALS)
        ));
        return Ok(());
    }

    manager.message(format!(
        "Sending {} UNO to {}",
        format_coin(amount, tos_common::config::COIN_DECIMALS),
        address
    ));

    // Build the UNO transfer
    manager.message("Building UNO transfer transaction...");

    let storage = wallet.get_storage().read().await;
    let light_api = wallet
        .get_light_api()
        .await
        .map_err(|e| CommandError::InvalidArgument(format!("Failed to get light API: {}", e)))?;

    // Query nonce and reference from daemon
    let nonce = light_api
        .get_next_nonce(&wallet_address)
        .await
        .map_err(|e| CommandError::InvalidArgument(format!("Failed to query nonce: {}", e)))?;

    let reference = light_api
        .get_reference_block()
        .await
        .map_err(|e| CommandError::InvalidArgument(format!("Failed to query reference: {}", e)))?;

    // Create transaction state with UNO balance
    let mut state =
        TransactionBuilderState::new(wallet.get_network().is_mainnet(), reference, nonce);

    // Add the UNO balance to state
    state.add_uno_balance(
        UNO_ASSET,
        UnoBalance::new(CiphertextCache::Decompressed(ciphertext), decrypted_amount),
    );

    // Create UNO transfer builder
    let uno_transfer = UnoTransferBuilder {
        asset: UNO_ASSET,
        amount,
        destination: address,
        extra_data: None,
        encrypt_extra_data: false,
    };
    let tx_type = TransactionTypeBuilder::UnoTransfers(vec![uno_transfer]);

    // Get TX version
    let tx_version = storage
        .get_tx_version()
        .await
        .context("Error while getting tx version")?;

    // Create transaction builder
    // UNO transfers use UNO_ASSET for fees (fee is paid from sender's remaining UNO balance)
    let builder = tos_common::transaction::builder::TransactionBuilder::new(
        tx_version,
        wallet.get_network().chain_id() as u8,
        wallet.get_public_key().clone(),
        None, // No multisig for UNO transfers
        tx_type,
        FeeBuilder::default(),
    );

    // Build the UNO transaction (requires keypair for ZK proof generation)
    let tx = match builder.build_uno_unsigned(&mut state, wallet.get_keypair()) {
        Ok(unsigned_tx) => {
            // Sign and finalize the transaction
            unsigned_tx.finalize(wallet.get_keypair())
        }
        Err(e) => {
            manager.error(format!("Error while creating UNO transaction: {}", e));
            return Ok(());
        }
    };

    manager.message("UNO transfer transaction created successfully!");

    // Release the lock before broadcasting
    drop(storage);
    drop(network_handler);

    broadcast_tx(wallet, manager, tx).await;
    Ok(())
}

/// Shield transfer: TOS (plaintext) -> UNO (encrypted privacy balance)
async fn shield_transfer(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    use tos_common::config::TOS_ASSET;
    use tos_common::transaction::builder::ShieldTransferBuilder;
    use tos_wallet::transaction_builder::TransactionBuilderState;

    manager.validate_batch_params("shield_transfer", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Parse address
    let str_address = args.get_value("address")?.to_string_value()?;
    let address = Address::from_string(&str_address).context("Invalid address")?;

    // Parse amount
    let amount_str = args.get_value("amount")?.to_string_value()?;
    let amount =
        from_coin(amount_str, tos_common::config::COIN_DECIMALS).context("Invalid amount")?;

    if amount == 0 {
        manager.error("Amount must be greater than 0");
        return Ok(());
    }

    manager.message(format!(
        "Shielding {} TOS to {}",
        format_coin(amount, tos_common::config::COIN_DECIMALS),
        address
    ));

    // Get network handler
    let network_handler = wallet.get_network_handler().lock().await;
    let handler = network_handler.as_ref().ok_or_else(|| {
        CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
    })?;
    let wallet_address = wallet.get_address();

    // Query nonce and reference from daemon
    let light_api = wallet
        .get_light_api()
        .await
        .map_err(|e| CommandError::InvalidArgument(format!("Failed to get light API: {}", e)))?;

    let nonce = light_api
        .get_next_nonce(&wallet_address)
        .await
        .map_err(|e| CommandError::InvalidArgument(format!("Failed to query nonce: {}", e)))?;

    let reference = light_api
        .get_reference_block()
        .await
        .map_err(|e| CommandError::InvalidArgument(format!("Failed to query reference: {}", e)))?;

    // Check TOS balance
    let balance = handler
        .get_api()
        .get_balance(&wallet_address, &TOS_ASSET)
        .await
        .map_err(|e| CommandError::InvalidArgument(format!("Failed to get balance: {}", e)))?
        .balance;

    // Estimate fee
    let estimated_fee = 1000u64; // Base fee estimate
    if balance < amount + estimated_fee {
        manager.error(format!(
            "Insufficient TOS balance. Have: {}, Need: {} (amount) + fees",
            format_coin(balance, tos_common::config::COIN_DECIMALS),
            format_coin(amount, tos_common::config::COIN_DECIMALS)
        ));
        return Ok(());
    }

    // Create transaction state
    let storage = wallet.get_storage().read().await;
    let mut state =
        TransactionBuilderState::new(wallet.get_network().is_mainnet(), reference, nonce);

    // Add TOS balance to state
    use tos_wallet::transaction_builder::Balance;
    state.add_balance(TOS_ASSET, Balance::new(balance));

    // Create Shield transfer builder
    let shield_transfer = ShieldTransferBuilder::new(TOS_ASSET, amount, address);
    let tx_type = TransactionTypeBuilder::ShieldTransfers(vec![shield_transfer]);

    // Get TX version
    let tx_version = storage
        .get_tx_version()
        .await
        .context("Error while getting tx version")?;

    // Create transaction builder
    let builder = tos_common::transaction::builder::TransactionBuilder::new(
        tx_version,
        wallet.get_network().chain_id() as u8,
        wallet.get_public_key().clone(),
        None,
        tx_type,
        FeeBuilder::default(),
    );

    // Build the Shield transaction
    let tx = match builder.build_shield_unsigned(&mut state, wallet.get_keypair()) {
        Ok(unsigned_tx) => unsigned_tx.finalize(wallet.get_keypair()),
        Err(e) => {
            manager.error(format!("Error while creating Shield transaction: {}", e));
            return Ok(());
        }
    };

    manager.message("Shield transaction created successfully!");

    // Release locks before broadcasting
    drop(storage);
    drop(network_handler);

    broadcast_tx(wallet, manager, tx).await;
    Ok(())
}

/// Unshield transfer: UNO (encrypted privacy balance) -> TOS (plaintext)
async fn unshield_transfer(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    use tos_common::account::CiphertextCache;
    use tos_common::config::{TOS_ASSET, UNO_ASSET};
    use tos_common::transaction::builder::UnshieldTransferBuilder;
    use tos_wallet::transaction_builder::{TransactionBuilderState, UnoBalance};

    manager.validate_batch_params("unshield_transfer", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Parse address
    let str_address = args.get_value("address")?.to_string_value()?;
    let address = Address::from_string(&str_address).context("Invalid address")?;

    // Parse amount
    let amount_str = args.get_value("amount")?.to_string_value()?;
    let amount =
        from_coin(amount_str, tos_common::config::COIN_DECIMALS).context("Invalid amount")?;

    if amount == 0 {
        manager.error("Amount must be greater than 0");
        return Ok(());
    }

    // Get network handler
    let network_handler = wallet.get_network_handler().lock().await;
    let handler = network_handler.as_ref().ok_or_else(|| {
        CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
    })?;
    let wallet_address = wallet.get_address();

    // Check if account has UNO balance
    let has_uno = handler
        .get_api()
        .has_uno_balance(&wallet_address)
        .await
        .map_err(|e| {
            CommandError::InvalidArgument(format!("Failed to check UNO balance: {}", e))
        })?;

    if !has_uno {
        manager.error("No UNO balance found for this address.");
        manager.message("Use 'shield_transfer' first to convert TOS to UNO.");
        return Ok(());
    }

    // Get and decrypt UNO balance
    let uno_result = handler
        .get_api()
        .get_uno_balance(&wallet_address)
        .await
        .map_err(|e| CommandError::InvalidArgument(format!("Failed to get UNO balance: {}", e)))?;

    manager.message("Decrypting UNO balance...");
    let mut versioned_balance = uno_result.version;
    let ciphertext = versioned_balance
        .get_mut_balance()
        .decompressed()
        .map_err(|e| {
            CommandError::InvalidArgument(format!("Failed to decompress ciphertext: {}", e))
        })?
        .clone();

    // Decrypt using ECDLP tables
    let precomputed_tables = wallet.get_precomputed_tables();
    let tables_guard = precomputed_tables.read().map_err(|e| {
        CommandError::InvalidArgument(format!("Failed to acquire ECDLP tables lock: {}", e))
    })?;
    let tables_view = tables_guard.view();
    let decrypted_uno_balance = wallet
        .get_keypair()
        .decrypt(&tables_view, &ciphertext)
        .ok_or_else(|| {
            CommandError::InvalidArgument(
                "Failed to decrypt UNO balance (value too large or corrupted)".to_string(),
            )
        })?;
    drop(tables_guard);

    manager.message(format!(
        "Current UNO balance: {} UNO",
        format_coin(decrypted_uno_balance, tos_common::config::COIN_DECIMALS)
    ));

    // Check sufficient UNO balance
    if decrypted_uno_balance < amount {
        manager.error(format!(
            "Insufficient UNO balance. Have: {}, Need: {}",
            format_coin(decrypted_uno_balance, tos_common::config::COIN_DECIMALS),
            format_coin(amount, tos_common::config::COIN_DECIMALS)
        ));
        return Ok(());
    }

    manager.message(format!(
        "Unshielding {} UNO to {} as TOS",
        format_coin(amount, tos_common::config::COIN_DECIMALS),
        address
    ));

    // Query nonce and reference
    let light_api = wallet
        .get_light_api()
        .await
        .map_err(|e| CommandError::InvalidArgument(format!("Failed to get light API: {}", e)))?;

    let nonce = light_api
        .get_next_nonce(&wallet_address)
        .await
        .map_err(|e| CommandError::InvalidArgument(format!("Failed to query nonce: {}", e)))?;

    let reference = light_api
        .get_reference_block()
        .await
        .map_err(|e| CommandError::InvalidArgument(format!("Failed to query reference: {}", e)))?;

    // Check TOS balance for fees
    let tos_balance = handler
        .get_api()
        .get_balance(&wallet_address, &TOS_ASSET)
        .await
        .map_err(|e| CommandError::InvalidArgument(format!("Failed to get TOS balance: {}", e)))?
        .balance;

    let estimated_fee = 1000u64; // Base fee estimate
    if tos_balance < estimated_fee {
        manager.error(format!(
            "Insufficient TOS balance for fees. Have: {}, Need: ~{}",
            format_coin(tos_balance, tos_common::config::COIN_DECIMALS),
            format_coin(estimated_fee, tos_common::config::COIN_DECIMALS)
        ));
        return Ok(());
    }

    // Create transaction state
    let storage = wallet.get_storage().read().await;
    let mut state =
        TransactionBuilderState::new(wallet.get_network().is_mainnet(), reference, nonce);

    // Add balances to state
    use tos_wallet::transaction_builder::Balance;
    state.add_balance(TOS_ASSET, Balance::new(tos_balance));
    state.add_uno_balance(
        UNO_ASSET,
        UnoBalance::new(
            CiphertextCache::Decompressed(ciphertext),
            decrypted_uno_balance,
        ),
    );

    // Create Unshield transfer builder
    let unshield_transfer = UnshieldTransferBuilder::new(UNO_ASSET, amount, address);
    let tx_type = TransactionTypeBuilder::UnshieldTransfers(vec![unshield_transfer]);

    // Get TX version
    let tx_version = storage
        .get_tx_version()
        .await
        .context("Error while getting tx version")?;

    // Create transaction builder
    let builder = tos_common::transaction::builder::TransactionBuilder::new(
        tx_version,
        wallet.get_network().chain_id() as u8,
        wallet.get_public_key().clone(),
        None,
        tx_type,
        FeeBuilder::default(),
    );

    // Build the Unshield transaction
    let tx = match builder.build_unshield_unsigned(&mut state, wallet.get_keypair()) {
        Ok(unsigned_tx) => unsigned_tx.finalize(wallet.get_keypair()),
        Err(e) => {
            manager.error(format!("Error while creating Unshield transaction: {}", e));
            return Ok(());
        }
    };

    manager.message("Unshield transaction created successfully!");

    // Release locks before broadcasting
    drop(storage);
    drop(network_handler);

    broadcast_tx(wallet, manager, tx).await;
    Ok(())
}

// Show all transactions from daemon
async fn history(
    manager: &CommandManager,
    mut arguments: ArgumentManager,
) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    {
        use tos_common::api::daemon::AccountHistoryType;

        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();
        let wallet_address = wallet.get_address();

        // Get asset filter if specified
        let asset = if arguments.has_argument("asset") {
            let asset_str = arguments.get_value("asset")?.to_string_value()?;
            if asset_str.is_empty() || asset_str.to_uppercase() == "TOS" {
                TOS_ASSET
            } else if asset_str.len() == HASH_SIZE * 2 {
                Hash::from_hex(&asset_str).context("Error while parsing asset hash from hex")?
            } else {
                return Err(CommandError::InvalidArgument(
                    "Invalid asset. Use 64-character hex hash or 'TOS'.".to_string(),
                ));
            }
        } else {
            TOS_ASSET
        };

        // Query history from daemon
        let history = daemon_api
            .get_account_history(&wallet_address, &asset, None, None)
            .await
            .map_err(|e| {
                CommandError::InvalidArgument(format!("Failed to get history from daemon: {}", e))
            })?;

        if history.is_empty() {
            manager.message("No transactions available");
            return Ok(());
        }

        // Pagination
        let page = if arguments.has_argument("page") {
            arguments.get_value("page")?.to_number()? as usize
        } else {
            1
        };

        if page == 0 {
            return Err(CommandError::InvalidArgument(
                "Page must be greater than 0".to_string(),
            ));
        }

        let txs_len = history.len();
        let mut max_pages = txs_len / ELEMENTS_PER_PAGE;
        if txs_len % ELEMENTS_PER_PAGE != 0 {
            max_pages += 1;
        }

        if page > max_pages {
            return Err(CommandError::InvalidArgument(format!(
                "Page must be less than maximum pages ({})",
                max_pages
            )));
        }

        let start = (page - 1) * ELEMENTS_PER_PAGE;
        let end = std::cmp::min(start + ELEMENTS_PER_PAGE, txs_len);
        let page_entries = &history[start..end];

        manager.message(format!(
            "{} Transactions (total {}) page {}/{}:",
            page_entries.len(),
            txs_len,
            page,
            max_pages
        ));

        for entry in page_entries {
            let type_str = match &entry.history_type {
                AccountHistoryType::Mining { reward } => {
                    format!("Mining reward: {} TOS", format_tos(*reward))
                }
                AccountHistoryType::DevFee { reward } => {
                    format!("Dev fee: {} TOS", format_tos(*reward))
                }
                AccountHistoryType::Burn { amount } => {
                    format!("Burn: {} TOS", format_tos(*amount))
                }
                AccountHistoryType::Outgoing { to } => {
                    format!("Sent to {}", to)
                }
                AccountHistoryType::Incoming { from } => {
                    format!("Received from {}", from)
                }
                AccountHistoryType::MultiSig {
                    participants,
                    threshold,
                } => {
                    format!(
                        "Multisig setup: {}/{} participants",
                        threshold,
                        participants.len()
                    )
                }
                AccountHistoryType::InvokeContract { contract, entry_id } => {
                    format!("Contract call: {} (entry {})", contract, entry_id)
                }
                AccountHistoryType::DeployContract => "Contract deployed".to_string(),
                AccountHistoryType::FreezeTos { amount, duration } => {
                    format!("Freeze: {} TOS for {}", format_tos(*amount), duration)
                }
                AccountHistoryType::UnfreezeTos { amount } => {
                    format!("Unfreeze: {} TOS", format_tos(*amount))
                }
                AccountHistoryType::BindReferrer { referrer } => {
                    format!("Bind referrer: {}", referrer)
                }
            };

            manager.message(format!(
                "- [{}] {} | TX: {}",
                entry.topoheight, type_str, entry.hash
            ));
        }
    }

    Ok(())
}

async fn transaction(
    manager: &CommandManager,
    mut arguments: ArgumentManager,
) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let hash = arguments.get_value("hash")?.to_hash()?;

    {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();

        // Query transaction from daemon
        let tx = daemon_api
            .get_transaction(&hash)
            .await
            .map_err(|e| CommandError::InvalidArgument(format!("Transaction not found: {}", e)))?;

        // Display transaction details
        manager.message(format!("Transaction: {}", hash));
        manager.message(format!("Version: {}", tx.get_version()));
        manager.message(format!(
            "Source: {}",
            tx.get_source()
                .as_address(wallet.get_network().is_mainnet())
        ));
        manager.message(format!("Nonce: {}", tx.get_nonce()));
        manager.message(format!("Fee: {} TOS", format_tos(tx.get_fee())));
        manager.message(format!("Reference: {}", tx.get_reference()));

        // Display transaction type details
        use tos_common::transaction::{AgentAccountPayload, TransactionType};
        match tx.get_data() {
            TransactionType::Transfers(transfers) => {
                manager.message(format!("Type: Transfers ({} outputs)", transfers.len()));
                for (i, transfer) in transfers.iter().enumerate() {
                    let dest = transfer
                        .get_destination()
                        .as_address(wallet.get_network().is_mainnet());
                    let amount = transfer.get_amount();
                    let asset = transfer.get_asset();
                    if *asset == TOS_ASSET {
                        manager.message(format!(
                            "  [{}] {} TOS -> {}",
                            i,
                            format_tos(amount),
                            dest
                        ));
                    } else {
                        manager.message(format!(
                            "  [{}] {} (asset: {}) -> {}",
                            i, amount, asset, dest
                        ));
                    }
                }
            }
            TransactionType::Burn(payload) => {
                manager.message("Type: Burn".to_string());
                manager.message(format!("  Amount: {}", payload.amount));
                manager.message(format!("  Asset: {}", payload.asset));
            }
            TransactionType::MultiSig(payload) => {
                manager.message("Type: MultiSig".to_string());
                manager.message(format!("  Threshold: {}", payload.threshold));
                manager.message(format!("  Participants: {}", payload.participants.len()));
            }
            TransactionType::InvokeContract(payload) => {
                manager.message("Type: InvokeContract".to_string());
                manager.message(format!("  Contract: {}", payload.contract));
            }
            TransactionType::DeployContract(_) => {
                manager.message("Type: DeployContract".to_string());
            }
            TransactionType::Energy(payload) => {
                use tos_common::transaction::EnergyPayload;
                match payload {
                    EnergyPayload::FreezeTos { amount, duration } => {
                        manager.message("Type: FreezeTos".to_string());
                        manager.message(format!("  Amount: {} TOS", format_tos(*amount)));
                        manager.message(format!("  Duration: {:?}", duration));
                    }
                    EnergyPayload::FreezeTosDelegate {
                        delegatees,
                        duration,
                    } => {
                        manager.message("Type: FreezeTosDelegate".to_string());
                        manager.message(format!("  Delegatees: {} accounts", delegatees.len()));
                        manager.message(format!("  Duration: {:?}", duration));
                    }
                    EnergyPayload::UnfreezeTos {
                        amount,
                        from_delegation,
                        record_index,
                        delegatee_address,
                    } => {
                        manager.message("Type: UnfreezeTos".to_string());
                        manager.message(format!("  Amount: {} TOS", format_tos(*amount)));
                        manager.message(format!("  From Delegation: {}", from_delegation));
                        if let Some(idx) = record_index {
                            manager.message(format!("  Record Index: {}", idx));
                        }
                        if let Some(addr) = delegatee_address {
                            manager.message(format!("  Delegatee: {:?}", addr));
                        }
                    }
                    EnergyPayload::WithdrawUnfrozen => {
                        manager.message("Type: WithdrawUnfrozen".to_string());
                    }
                }
            }
            TransactionType::BindReferrer(payload) => {
                manager.message("Type: BindReferrer".to_string());
                manager.message(format!(
                    "  Referrer: {}",
                    payload
                        .get_referrer()
                        .as_address(wallet.get_network().is_mainnet())
                ));
            }
            TransactionType::BatchReferralReward(payload) => {
                manager.message("Type: BatchReferralReward".to_string());
                manager.message(format!("  Asset: {}", payload.get_asset()));
                manager.message(format!("  Total Amount: {}", payload.get_total_amount()));
                manager.message(format!("  Levels: {}", payload.get_levels()));
            }
            TransactionType::AgentAccount(payload) => {
                manager.message("Type: AgentAccount".to_string());
                match payload {
                    AgentAccountPayload::Register {
                        controller,
                        policy_hash,
                        energy_pool,
                        session_key_root,
                    } => {
                        manager.message("  Action: Register".to_string());
                        manager.message(format!(
                            "  Controller: {}",
                            controller.as_address(wallet.get_network().is_mainnet())
                        ));
                        manager.message(format!("  Policy Hash: {}", policy_hash));
                        if let Some(pool) = energy_pool.as_ref() {
                            manager.message(format!(
                                "  Energy Pool: {}",
                                pool.as_address(wallet.get_network().is_mainnet())
                            ));
                        }
                        if let Some(root) = session_key_root.as_ref() {
                            manager.message(format!("  Session Key Root: {}", root));
                        }
                    }
                    AgentAccountPayload::UpdatePolicy { policy_hash } => {
                        manager.message("  Action: UpdatePolicy".to_string());
                        manager.message(format!("  Policy Hash: {}", policy_hash));
                    }
                    AgentAccountPayload::RotateController { new_controller } => {
                        manager.message("  Action: RotateController".to_string());
                        manager.message(format!(
                            "  New Controller: {}",
                            new_controller.as_address(wallet.get_network().is_mainnet())
                        ));
                    }
                    AgentAccountPayload::SetStatus { status } => {
                        manager.message("  Action: SetStatus".to_string());
                        manager.message(format!("  Status: {}", status));
                    }
                    AgentAccountPayload::SetEnergyPool { energy_pool } => {
                        manager.message("  Action: SetEnergyPool".to_string());
                        if let Some(pool) = energy_pool.as_ref() {
                            manager.message(format!(
                                "  Energy Pool: {}",
                                pool.as_address(wallet.get_network().is_mainnet())
                            ));
                        } else {
                            manager.message("  Energy Pool: none".to_string());
                        }
                    }
                    AgentAccountPayload::SetSessionKeyRoot { session_key_root } => {
                        manager.message("  Action: SetSessionKeyRoot".to_string());
                        if let Some(root) = session_key_root.as_ref() {
                            manager.message(format!("  Session Key Root: {}", root));
                        } else {
                            manager.message("  Session Key Root: none".to_string());
                        }
                    }
                    AgentAccountPayload::AddSessionKey { key } => {
                        manager.message("  Action: AddSessionKey".to_string());
                        manager.message(format!("  Key ID: {}", key.key_id));
                        manager.message(format!(
                            "  Public Key: {}",
                            key.public_key.as_address(wallet.get_network().is_mainnet())
                        ));
                        manager.message(format!("  Expiry Topoheight: {}", key.expiry_topoheight));
                    }
                    AgentAccountPayload::RevokeSessionKey { key_id } => {
                        manager.message("  Action: RevokeSessionKey".to_string());
                        manager.message(format!("  Key ID: {}", key_id));
                    }
                }
            }
            TransactionType::SetKyc(payload) => {
                manager.message("Type: SetKyc".to_string());
                manager.message(format!(
                    "  Account: {}",
                    payload
                        .get_account()
                        .as_address(wallet.get_network().is_mainnet())
                ));
                manager.message(format!("  Level: {}", payload.get_level()));
            }
            TransactionType::RevokeKyc(payload) => {
                manager.message("Type: RevokeKyc".to_string());
                manager.message(format!(
                    "  Account: {}",
                    payload
                        .get_account()
                        .as_address(wallet.get_network().is_mainnet())
                ));
                manager.message(format!("  Reason Hash: {}", payload.get_reason_hash()));
            }
            TransactionType::RenewKyc(payload) => {
                manager.message("Type: RenewKyc".to_string());
                manager.message(format!(
                    "  Account: {}",
                    payload
                        .get_account()
                        .as_address(wallet.get_network().is_mainnet())
                ));
            }
            TransactionType::BootstrapCommittee(payload) => {
                manager.message("Type: BootstrapCommittee".to_string());
                manager.message(format!("  Name: {}", payload.get_name()));
                manager.message(format!("  Members: {}", payload.get_members().len()));
            }
            TransactionType::RegisterCommittee(payload) => {
                manager.message("Type: RegisterCommittee".to_string());
                manager.message(format!("  Region: {}", payload.get_region()));
                manager.message(format!("  Name: {}", payload.get_name()));
            }
            TransactionType::UpdateCommittee(payload) => {
                manager.message("Type: UpdateCommittee".to_string());
                manager.message(format!("  Committee: {}", payload.get_committee_id()));
            }
            TransactionType::EmergencySuspend(payload) => {
                manager.message("Type: EmergencySuspend".to_string());
                manager.message(format!(
                    "  Account: {}",
                    payload
                        .get_account()
                        .as_address(wallet.get_network().is_mainnet())
                ));
                manager.message(format!("  Reason Hash: {}", payload.get_reason_hash()));
            }
            TransactionType::TransferKyc(payload) => {
                manager.message("Type: TransferKyc".to_string());
                manager.message(format!(
                    "  Account: {}",
                    payload
                        .get_account()
                        .as_address(wallet.get_network().is_mainnet())
                ));
                manager.message(format!(
                    "  Source Committee: {}",
                    payload.get_source_committee_id()
                ));
                manager.message(format!(
                    "  Dest Committee: {}",
                    payload.get_dest_committee_id()
                ));
                manager.message(format!(
                    "  Source Approvals: {}",
                    payload.get_source_approvals().len()
                ));
                manager.message(format!(
                    "  Dest Approvals: {}",
                    payload.get_dest_approvals().len()
                ));
            }
            TransactionType::AppealKyc(payload) => {
                manager.message("Type: AppealKyc".to_string());
                manager.message(format!(
                    "  Account: {}",
                    payload
                        .get_account()
                        .as_address(wallet.get_network().is_mainnet())
                ));
                manager.message(format!(
                    "  Original Committee: {}",
                    payload.get_original_committee_id()
                ));
                manager.message(format!(
                    "  Parent Committee: {}",
                    payload.get_parent_committee_id()
                ));
                manager.message(format!("  Reason Hash: {}", payload.get_reason_hash()));
                manager.message(format!("  Submitted At: {}", payload.get_submitted_at()));
            }
            TransactionType::UnoTransfers(transfers) => {
                manager.message("Type: UNO Transfers (Privacy-Preserving)");
                manager.message(format!("  Transfer Count: {}", transfers.len()));
                for (i, transfer) in transfers.iter().enumerate() {
                    manager.message(format!(
                        "  Transfer #{}: to {}",
                        i + 1,
                        transfer
                            .get_destination()
                            .as_address(wallet.get_network().is_mainnet())
                    ));
                    manager.message(format!("    Asset: {}", transfer.get_asset()));
                }
            }
            TransactionType::ShieldTransfers(transfers) => {
                manager.message("Type: Shield Transfers (TOS -> UNO)");
                manager.message(format!("  Transfer Count: {}", transfers.len()));
                for (i, transfer) in transfers.iter().enumerate() {
                    manager.message(format!(
                        "  Shield #{}: {} TOS -> {}",
                        i + 1,
                        format_tos(transfer.get_amount()),
                        transfer
                            .get_destination()
                            .as_address(wallet.get_network().is_mainnet())
                    ));
                }
            }
            TransactionType::UnshieldTransfers(transfers) => {
                manager.message("Type: Unshield Transfers (UNO -> TOS)");
                manager.message(format!("  Transfer Count: {}", transfers.len()));
                for (i, transfer) in transfers.iter().enumerate() {
                    manager.message(format!(
                        "  Unshield #{}: {} TOS -> {}",
                        i + 1,
                        format_tos(transfer.get_amount()),
                        transfer
                            .get_destination()
                            .as_address(wallet.get_network().is_mainnet())
                    ));
                }
            }
            TransactionType::RegisterName(payload) => {
                manager.message("Type: Register TNS Name");
                manager.message(format!("  Name: {}", payload.get_name()));
            }
            TransactionType::EphemeralMessage(payload) => {
                manager.message("Type: Ephemeral Message");
                manager.message(format!("  Sender Hash: {}", payload.get_sender_name_hash()));
                manager.message(format!(
                    "  Recipient Hash: {}",
                    payload.get_recipient_name_hash()
                ));
                manager.message(format!("  TTL Blocks: {}", payload.get_ttl_blocks()));
            }
        }
    }

    Ok(())
}

async fn export_transactions_csv(
    manager: &CommandManager,
    mut arguments: ArgumentManager,
) -> Result<(), CommandError> {
    let filename = arguments.get_value("filename")?.to_string_value()?;
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    {
        use tos_common::api::daemon::AccountHistoryType;

        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();
        let wallet_address = wallet.get_address();

        // Query history from daemon for TOS asset
        let history = daemon_api
            .get_account_history(&wallet_address, &TOS_ASSET, None, None)
            .await
            .map_err(|e| {
                CommandError::InvalidArgument(format!("Failed to get history from daemon: {}", e))
            })?;

        if history.is_empty() {
            manager.message("No transactions to export");
            return Ok(());
        }

        // Create CSV file
        let mut file = File::create(&filename).context("Error while creating CSV file")?;

        // Write CSV header
        writeln!(file, "topoheight,hash,type,details,timestamp")
            .context("Error writing CSV header")?;

        // Write each transaction
        for entry in &history {
            let type_str = match &entry.history_type {
                AccountHistoryType::Mining { reward } => {
                    format!("Mining,reward:{}", reward)
                }
                AccountHistoryType::DevFee { reward } => {
                    format!("DevFee,reward:{}", reward)
                }
                AccountHistoryType::Burn { amount } => {
                    format!("Burn,amount:{}", amount)
                }
                AccountHistoryType::Outgoing { to } => {
                    format!("Outgoing,to:{}", to)
                }
                AccountHistoryType::Incoming { from } => {
                    format!("Incoming,from:{}", from)
                }
                AccountHistoryType::MultiSig {
                    participants,
                    threshold,
                } => {
                    format!(
                        "MultiSig,threshold:{} participants:{}",
                        threshold,
                        participants.len()
                    )
                }
                AccountHistoryType::InvokeContract { contract, entry_id } => {
                    format!("InvokeContract,contract:{} entry:{}", contract, entry_id)
                }
                AccountHistoryType::DeployContract => "DeployContract,".to_string(),
                AccountHistoryType::FreezeTos { amount, duration } => {
                    format!("FreezeTos,amount:{} duration:{}", amount, duration)
                }
                AccountHistoryType::UnfreezeTos { amount } => {
                    format!("UnfreezeTos,amount:{}", amount)
                }
                AccountHistoryType::BindReferrer { referrer } => {
                    format!("BindReferrer,referrer:{}", referrer)
                }
            };

            writeln!(
                file,
                "{},{},{},{}",
                entry.topoheight, entry.hash, type_str, entry.block_timestamp
            )
            .context("Error writing CSV row")?;
        }

        manager.message(format!(
            "{} transactions have been exported to {}",
            history.len(),
            filename
        ));
    }

    Ok(())
}

// Show wallet connection status with daemon (stateless wallet)

async fn sync_status(
    manager: &CommandManager,
    _arguments: ArgumentManager,
) -> Result<(), CommandError> {
    let wallet = {
        let context = manager.get_context().lock()?;
        context.get::<Arc<Wallet>>()?.clone()
    };

    // Stateless wallet: check daemon connection status
    let is_online = wallet.is_online().await;

    if is_online {
        match wallet.get_sync_progress().await {
            Ok((_, daemon_topo, _)) => {
                manager.message(format!(
                    "Stateless wallet: Connected to daemon\n  Daemon topoheight: {daemon_topo}\n  Status: Ready (queries on-demand)"
                ));
            }
            Err(e) => {
                manager.message(format!(
                    "Stateless wallet: Connected but error getting info: {e:#}"
                ));
            }
        }
    } else {
        manager.message("Stateless wallet: Offline (not connected to daemon)");
    }

    Ok(())
}

async fn seed(
    manager: &CommandManager,
    mut arguments: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("seed", &arguments)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Password required (batch mode only)
    let password = if arguments.has_argument("password") {
        arguments.get_value("password")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument("password".to_string()));
    };

    // check if password is valid
    wallet.is_valid_password(&password).await?;

    let language = if arguments.has_argument("language") {
        arguments.get_value("language")?.to_number()?
    } else {
        0
    };

    let seed = wallet.get_seed(language as usize)?;
    manager.message(format!("Seed: {}", seed));
    Ok(())
}

// Show private key in hex format for backup/recovery
async fn show_private_key(
    manager: &CommandManager,
    mut arguments: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("private_key", &arguments)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Password required (batch mode only)
    let password = arguments.get_value("password")?.to_string_value()?;

    // Check if password is valid
    wallet.is_valid_password(&password).await?;

    let private_key_hex = wallet.get_keypair().get_private_key().to_hex();
    manager.message(format!("Private Key: {}", private_key_hex));
    manager.message("WARNING: Never share your private key with anyone!");
    Ok(())
}

// NOTE: This command now queries 100% from daemon API (no local storage)
async fn nonce(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Query nonce from daemon API (stateless - no local storage)

    let nonce = {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let address = wallet.get_address();
        let result = handler.get_api().get_nonce(&address).await.map_err(|e| {
            CommandError::InvalidArgument(format!("Failed to get nonce from daemon: {}", e))
        })?;
        result.version.get_nonce()
    };

    manager.message(format!("Nonce: {}", nonce));
    Ok(())
}

async fn tx_version(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let storage = wallet.get_storage().read().await;
    let version = storage.get_tx_version().await?;
    manager.message(format!("Transaction version: {}", version));
    Ok(())
}

async fn set_tx_version(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    // Version required (batch mode only)
    let value: u8 = if args.has_argument("version") {
        args.get_value("version")?
            .to_number()?
            .try_into()
            .map_err(|_| CommandError::InvalidArgument("Invalid transaction version".to_string()))?
    } else {
        return Err(CommandError::MissingArgument("version".to_string()));
    };

    let tx_version = TxVersion::try_from(value)
        .map_err(|_| CommandError::InvalidArgument("Invalid transaction version".to_string()))?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let mut storage = wallet.get_storage().write().await;
    storage.set_tx_version(tx_version).await?;

    manager.message(format!("New transaction version is: {}", value));
    Ok(())
}

async fn status(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    if let Some(network_handler) = wallet.get_network_handler().lock().await.as_ref() {
        let api = network_handler.get_api();
        let is_online = api.get_client().is_online();
        manager.message(format!("Network handler is online: {}", is_online));
        manager.message(format!("Connected to: {}", api.get_client().get_target()));

        if is_online {
            let info = api
                .get_info()
                .await
                .context("Error while getting network info")?;

            manager.message("--- Daemon status ---");
            manager.message(format!("Height: {}", info.height));
            manager.message(format!("Topoheight: {}", info.topoheight));
            manager.message(format!("Stable height: {}", info.stableheight));
            manager.message(format!("Pruned topoheight: {:?}", info.pruned_topoheight));
            manager.message(format!("Top block hash: {}", info.top_block_hash));
            manager.message(format!("Network: {}", info.network));
            manager.message(format!(
                "Emitted supply: {}",
                format_tos(info.emitted_supply)
            ));
            manager.message(format!("Burned supply: {}", format_tos(info.burned_supply)));
            manager.message(format!(
                "Circulating supply: {}",
                format_tos(info.circulating_supply)
            ));
            manager.message("---------------------");
        }
    }

    // Query multisig state from daemon

    {
        let network_handler = wallet.get_network_handler().lock().await;
        if let Some(handler) = network_handler.as_ref() {
            let daemon_api = handler.get_api();
            let wallet_address = wallet.get_address();

            // Query multisig from daemon
            match daemon_api.has_multisig(&wallet_address).await {
                Ok(true) => {
                    if let Ok(multisig) = daemon_api.get_multisig(&wallet_address).await {
                        use tos_common::api::daemon::MultisigState;
                        match multisig.state {
                            MultisigState::Active {
                                threshold,
                                participants,
                            } => {
                                manager.message("--- Multisig: ---");
                                manager.message(format!("Threshold: {}", threshold));
                                manager.message(format!(
                                    "Participants ({}): {}",
                                    participants.len(),
                                    participants
                                        .iter()
                                        .map(|p| p.to_string())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ));
                                manager.message("---------------");
                            }
                            MultisigState::Deleted => {
                                manager.message("Multisig: Deleted");
                            }
                        }
                    }
                }
                Ok(false) => {
                    manager.message("No multisig state");
                }
                Err(e) => {
                    manager.warn(format!("Could not query multisig state: {}", e));
                }
            }

            // Query nonce from daemon
            match daemon_api.get_nonce(&wallet_address).await {
                Ok(nonce_result) => {
                    manager.message(format!("Nonce: {}", nonce_result.version.get_nonce()));
                }
                Err(e) => {
                    manager.warn(format!("Could not query nonce: {}", e));
                }
            }
        } else {
            manager.message("No multisig state (not connected to daemon)");
            manager.message("Nonce: (not connected to daemon)");
        }
    }

    let storage = wallet.get_storage().read().await;
    let tx_version = storage.get_tx_version().await?;
    manager.message(format!("Transaction version: {}", tx_version));

    let network = wallet.get_network();
    manager.message(format!("Network: {}", network));
    manager.message(format!("Wallet address: {}", wallet.get_address()));

    Ok(())
}

async fn logout(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    {
        let context = manager.get_context().lock()?;
        let wallet: &Arc<Wallet> = context.get()?;
        wallet.close().await;
    }

    manager
        .remove_all_commands()
        .context("Error while removing all commands")?;
    manager.remove_from_context::<Arc<Wallet>>()?;

    register_default_commands(manager).await?;
    manager.message("Wallet has been closed");

    Ok(())
}

#[cfg(feature = "api_server")]
async fn stop_api_server(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    wallet
        .stop_api_server()
        .await
        .context("Error while stopping API Server")?;
    manager.message("API Server has been stopped");
    Ok(())
}

#[cfg(feature = "api_server")]
async fn start_rpc_server(
    manager: &CommandManager,
    mut arguments: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("start_rpc_server", &arguments)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let bind_address = arguments.get_value("bind_address")?.to_string_value()?;
    let username = arguments.get_value("username")?.to_string_value()?;
    let password = arguments.get_value("password")?.to_string_value()?;

    let auth_config = Some(AuthConfig { username, password });

    wallet
        .enable_rpc_server(bind_address, auth_config, None)
        .await
        .context("Error while enabling RPC Server")?;
    manager.message("RPC Server has been enabled");
    Ok(())
}

#[cfg(feature = "api_server")]
async fn start_xswd(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    match wallet.enable_xswd(None).await {
        Ok(receiver) => {
            if let Some(receiver) = receiver {
                let prompt = manager.get_prompt().clone();
                spawn_task("xswd", xswd_handler(receiver, prompt));
            }

            manager.message("XSWD Server has been enabled");
        }
        Err(e) => manager.error(format!("Error while enabling XSWD Server: {e}")),
    };

    Ok(())
}

#[cfg(feature = "xswd")]
async fn add_xswd_relayer(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // App data required (batch mode only)
    let app_data = if args.has_argument("app_data") {
        args.get_value("app_data")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument("app_data".to_string()));
    };

    let app_data =
        serde_json::from_str(&app_data).context("Error while parsing app data as JSON")?;

    match wallet.add_xswd_relayer(app_data).await {
        Ok(receiver) => {
            if let Some(receiver) = receiver {
                let prompt = manager.get_prompt().clone();
                spawn_task("xswd", xswd_handler(receiver, prompt));
            }

            manager.message("XSWD Server has been enabled");
        }
        Err(e) => manager.error(format!("Error while enabling XSWD Server: {e}")),
    };

    Ok(())
}

// Setup a multisig transaction
// Batch mode: multisig_setup threshold=2 addresses=addr1,addr2,addr3
async fn multisig_setup(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("multisig_setup", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Query multisig state from daemon (stateless wallet)
    let multisig = {
        let network_handler = wallet.get_network_handler().lock().await;
        if let Some(handler) = network_handler.as_ref() {
            let daemon_api = handler.get_api();
            let wallet_address = wallet.get_address();
            if daemon_api
                .has_multisig(&wallet_address)
                .await
                .unwrap_or(false)
            {
                daemon_api
                    .get_multisig(&wallet_address)
                    .await
                    .ok()
                    .map(|r| r.state)
            } else {
                None
            }
        } else {
            return Err(CommandError::InvalidArgument(
                "Wallet not connected to daemon".to_string(),
            ));
        }
    };

    // Get threshold (required)
    let threshold: u8 = if args.has_argument("threshold") {
        args.get_value("threshold")?.to_number()? as u8
    } else {
        return Err(CommandError::MissingArgument("threshold".to_string()));
    };

    // Handle delete case (threshold=0)
    if threshold == 0 {
        use tos_common::api::daemon::MultisigState;

        let Some(multisig) = multisig else {
            return Err(CommandError::InvalidArgument(
                "No multisig to delete".to_string(),
            ));
        };

        // Convert MultisigState to MultiSigPayload for delete operation
        let current_multisig = match multisig {
            MultisigState::Active {
                participants: p,
                threshold: t,
            } => MultiSigPayload {
                threshold: t,
                participants: p
                    .into_iter()
                    .map(|addr| addr.get_public_key().clone())
                    .collect(),
            },
            MultisigState::Deleted => {
                return Err(CommandError::InvalidArgument(
                    "Multisig is already deleted".to_string(),
                ));
            }
        };

        manager.message("Deleting multisig...");

        let payload = MultiSigBuilder {
            participants: IndexSet::new(),
            threshold: 0,
        };

        let tx = create_transaction_with_multisig(
            manager,
            manager.get_prompt(),
            wallet,
            TransactionTypeBuilder::MultiSig(payload),
            current_multisig,
        )
        .await?;

        broadcast_tx(wallet, manager, tx).await;
        return Ok(());
    }

    // Get addresses (required for setup)
    let addresses_str = if args.has_argument("addresses") {
        args.get_value("addresses")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument(
            "addresses (comma-separated list of participant addresses)".to_string(),
        ));
    };

    // Parse comma-separated addresses
    let mainnet = wallet.get_network().is_mainnet();
    let mut keys = IndexSet::new();
    for addr_str in addresses_str.split(',') {
        let addr_str = addr_str.trim();
        if addr_str.is_empty() {
            continue;
        }

        let address: Address = addr_str
            .parse()
            .map_err(|_| CommandError::InvalidArgument(format!("Invalid address: {}", addr_str)))?;

        if address.is_mainnet() != mainnet {
            return Err(CommandError::InvalidArgument(
                "Participant address must be on the same network".to_string(),
            ));
        }

        if !address.is_normal() {
            return Err(CommandError::InvalidArgument(
                "Participant address must be a normal address".to_string(),
            ));
        }

        if address.get_public_key() == wallet.get_public_key() {
            return Err(CommandError::InvalidArgument(
                "Participant address cannot be the same as the wallet address".to_string(),
            ));
        }

        if !keys.insert(address) {
            return Err(CommandError::InvalidArgument(
                "Duplicate participant address".to_string(),
            ));
        }
    }

    let participants = keys.len() as u8;
    if participants == 0 {
        return Err(CommandError::InvalidArgument(
            "At least one participant address required".to_string(),
        ));
    }

    if threshold > participants {
        return Err(CommandError::InvalidArgument(
            "Threshold must be less or equal to participants count".to_string(),
        ));
    }

    manager.message(format!(
        "MultiSig payload ({} participants with threshold at {}):",
        participants, threshold
    ));
    for key in keys.iter() {
        manager.message(format!("- {}", key));
    }

    manager.message("Building transaction...");

    // Stateless wallet: Query existing multisig from daemon
    let existing_multisig = {
        let network_handler = wallet.get_network_handler().lock().await;
        if let Some(handler) = network_handler.as_ref() {
            let daemon_api = handler.get_api();
            let wallet_address = wallet.get_address();
            match daemon_api.get_multisig(&wallet_address).await {
                Ok(multisig) => {
                    use indexmap::IndexSet;
                    use tos_common::api::daemon::MultisigState;
                    match multisig.state {
                        MultisigState::Active {
                            threshold: existing_threshold,
                            participants: existing_participants,
                        } => {
                            // Convert Vec<Address> to IndexSet<CompressedPublicKey>
                            // Note: PublicKey = CompressedPublicKey in this context
                            let compressed_keys: IndexSet<_> = existing_participants
                                .into_iter()
                                .map(|addr| addr.to_public_key())
                                .collect();
                            Some(tos_common::transaction::MultiSigPayload {
                                participants: compressed_keys,
                                threshold: existing_threshold,
                            })
                        }
                        MultisigState::Deleted => None,
                    }
                }
                Err(_) => None,
            }
        } else {
            None
        }
    };
    let payload = MultiSigBuilder {
        participants: keys,
        threshold,
    };
    let tx_type = TransactionTypeBuilder::MultiSig(payload);
    let prompt = manager.get_prompt();
    let tx = if let Some(multisig_payload) = existing_multisig {
        create_transaction_with_multisig(manager, prompt, wallet, tx_type, multisig_payload).await?
    } else {
        match wallet
            .create_transaction(tx_type, FeeBuilder::default())
            .await
        {
            Ok(tx) => tx,
            Err(e) => {
                manager.error(format!("Error while creating transaction: {}", e));
                return Ok(());
            }
        }
    };

    broadcast_tx(wallet, manager, tx).await;

    Ok(())
}

async fn multisig_sign(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    use tos_common::api::daemon::MultisigState;
    use tos_common::transaction::builder::UnsignedTransaction;
    use tos_common::transaction::multisig::{MultiSig, SignatureId};

    manager.validate_batch_params("multisig_sign", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // tx_hash required (batch mode only)
    let tx_hash = if args.has_argument("tx_hash") {
        args.get_value("tx_hash")?.to_hash()?
    } else {
        return Err(CommandError::MissingArgument("tx_hash".to_string()));
    };

    // Optional parameters
    let source_address =
        if args.has_argument("source") {
            let source_str = args.get_value("source")?.to_string_value()?;
            Some(Address::from_string(&source_str).map_err(|e| {
                CommandError::InvalidArgument(format!("Invalid source address: {}", e))
            })?)
        } else {
            None
        };

    let tx_data = if args.has_argument("tx_data") {
        Some(args.get_value("tx_data")?.to_string_value()?)
    } else {
        None
    };

    let other_signatures = if args.has_argument("signatures") {
        Some(args.get_value("signatures")?.to_string_value()?)
    } else {
        None
    };

    let should_submit = args.has_argument("submit") && args.get_flag("submit").unwrap_or(false);

    // Query multisig state from daemon to get signer ID
    // If source is provided, use that address (for participant wallets)
    // Otherwise use the current wallet's address (for multisig owner)
    let network_handler = wallet.get_network_handler().lock().await;
    let (participants, threshold) = if let Some(handler) = network_handler.as_ref() {
        let daemon_api = handler.get_api();
        let multisig_owner = source_address
            .clone()
            .unwrap_or_else(|| wallet.get_address());

        match daemon_api.get_multisig(&multisig_owner).await {
            Ok(multisig) => match multisig.state {
                MultisigState::Active {
                    threshold,
                    participants,
                } => (participants, threshold),
                MultisigState::Deleted => {
                    return Err(CommandError::InvalidArgument(
                        "Multisig has been deleted".to_string(),
                    ));
                }
            },
            Err(e) => {
                let error_msg = format!("{:#}", e);
                if error_msg.contains("not found") || error_msg.contains("No multisig") {
                    return Err(CommandError::InvalidArgument(
                        "No multisig configured for this wallet".to_string(),
                    ));
                }
                return Err(CommandError::InvalidArgument(format!(
                    "Could not query multisig state: {}",
                    e
                )));
            }
        }
    } else {
        return Err(CommandError::InvalidArgument(
            "Not connected to daemon".to_string(),
        ));
    };
    drop(network_handler);

    // Check if wallet is a participant or the source (owner)
    let wallet_address = wallet.get_address();
    let is_source = source_address.is_none(); // If no source provided, this wallet is the source
    let signer_id_opt = participants.iter().position(|p| p == &wallet_address);

    // If submitting with all signatures provided, we don't need to be a participant
    // The source wallet can submit on behalf of all participants
    if should_submit && other_signatures.is_some() {
        // Source wallet submitting with collected signatures
        // We might not be a participant, but we can still submit if we're the source
        if signer_id_opt.is_none() && !is_source {
            return Err(CommandError::InvalidArgument(
                "This wallet is not a participant in the multisig and not the source wallet"
                    .to_string(),
            ));
        }
    }

    // Sign the transaction hash if we're a participant
    let (signer_id, signature) = if let Some(id) = signer_id_opt {
        let sig = wallet.sign_data(tx_hash.as_bytes());
        (Some(id as u8), Some(sig))
    } else {
        (None, None)
    };

    manager.message(format!("Multisig threshold: {}", threshold));
    if let (Some(id), Some(sig)) = (signer_id, &signature) {
        manager.message(format!("Signer ID: {}", id));
        manager.message(format!("Signature: {}", sig.to_hex()));
        manager.message(format!("Combined format: {}:{}", id, sig.to_hex()));
    } else {
        manager.message(
            "This wallet is the source (not a participant), submitting collected signatures...",
        );
    }

    // If submit requested, build and submit the transaction
    if should_submit {
        let tx_data = tx_data.ok_or_else(|| {
            CommandError::MissingArgument("tx_data is required when submit=true".to_string())
        })?;

        // Deserialize unsigned transaction
        let mut unsigned = UnsignedTransaction::from_hex(&tx_data)
            .map_err(|e| CommandError::InvalidArgument(format!("Invalid tx_data hex: {}", e)))?;

        // Create multisig and add this wallet's signature (if participant)
        let mut multisig = MultiSig::new();
        if let (Some(id), Some(sig)) = (signer_id, signature.clone()) {
            if !multisig.add_signature(SignatureId { id, signature: sig }) {
                return Err(CommandError::InvalidArgument(
                    "Failed to add signature".to_string(),
                ));
            }
        }

        // Parse and add other signatures if provided
        if let Some(sigs_str) = other_signatures {
            for sig_part in sigs_str.split(',') {
                let parts: Vec<&str> = sig_part.trim().split(':').collect();
                if parts.len() != 2 {
                    return Err(CommandError::InvalidArgument(format!(
                        "Invalid signature format '{}', expected 'id:signature_hex'",
                        sig_part
                    )));
                }
                let id: u8 = parts[0].parse().map_err(|_| {
                    CommandError::InvalidArgument(format!("Invalid signer ID: {}", parts[0]))
                })?;
                let sig = Signature::from_hex(parts[1]).map_err(|e| {
                    CommandError::InvalidArgument(format!("Invalid signature hex: {}", e))
                })?;

                if !multisig.add_signature(SignatureId { id, signature: sig }) {
                    manager.warn(format!(
                        "Duplicate signature for signer ID {}, skipping",
                        id
                    ));
                }
            }
        }

        // Check if we have enough signatures
        let sig_count = multisig.len();
        if sig_count < threshold as usize {
            return Err(CommandError::InvalidArgument(format!(
                "Not enough signatures: have {}, need {}",
                sig_count, threshold
            )));
        }

        // Set multisig on unsigned transaction and finalize
        unsigned.set_multisig(multisig);
        let tx = unsigned.finalize(wallet.get_keypair());

        manager.message(format!("Transaction hash: {}", tx.hash()));
        manager.message("Submitting transaction...");

        // Submit to daemon
        let network_handler = wallet.get_network_handler().lock().await;
        if let Some(handler) = network_handler.as_ref() {
            let daemon_api = handler.get_api();
            match daemon_api.submit_transaction(&tx).await {
                Ok(()) => {
                    manager.message("Transaction submitted successfully!");
                }
                Err(e) => {
                    manager.error(format!("Failed to submit transaction: {}", e));
                }
            }
        } else {
            return Err(CommandError::InvalidArgument(
                "Not connected to daemon".to_string(),
            ));
        }
    }

    Ok(())
}

async fn multisig_show(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Stateless wallet: Query multisig state from daemon
    let network_handler = wallet.get_network_handler().lock().await;
    if let Some(handler) = network_handler.as_ref() {
        let daemon_api = handler.get_api();
        let wallet_address = wallet.get_address();

        match daemon_api.has_multisig(&wallet_address).await {
            Ok(true) => match daemon_api.get_multisig(&wallet_address).await {
                Ok(multisig) => {
                    use tos_common::api::daemon::MultisigState;
                    match multisig.state {
                        MultisigState::Active {
                            threshold,
                            participants,
                        } => {
                            manager.message(format!(
                                "MultiSig payload ({} participants with threshold at {}):",
                                participants.len(),
                                threshold
                            ));
                            for addr in participants.iter() {
                                manager.message(format!("- {}", addr));
                            }
                        }
                        MultisigState::Deleted => {
                            manager.message("Multisig: Deleted");
                        }
                    }
                }
                Err(e) => {
                    manager.error(format!("Could not query multisig state: {}", e));
                }
            },
            Ok(false) => {
                manager.message("No multisig configured");
            }
            Err(e) => {
                manager.error(format!("Could not query multisig state: {}", e));
            }
        }
    } else {
        manager.error("Not connected to daemon. Use 'online_mode' to connect first.");
    }

    Ok(())
}

// Create an unsigned transaction for multisig signing
// Outputs tx_hash (for signing) and tx_data (for reconstruction)
async fn multisig_create_tx(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    use tos_common::serializer::Serializer;
    use tos_common::transaction::builder::TransferBuilder;

    manager.validate_batch_params("multisig_create_tx", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Get parameters
    let asset_str = args.get_value("asset")?.to_string_value()?;
    let amount_str = args.get_value("amount")?.to_string_value()?;
    let address_str = args.get_value("address")?.to_string_value()?;

    // Parse asset
    let asset = if asset_str.to_lowercase() == "tos" {
        Hash::zero()
    } else {
        Hash::from_hex(&asset_str)
            .map_err(|e| CommandError::InvalidArgument(format!("Invalid asset hash: {}", e)))?
    };

    // Parse amount
    let amount = from_coin(&amount_str, 8)
        .ok_or_else(|| CommandError::InvalidArgument(format!("Invalid amount: {}", amount_str)))?;

    // Parse address
    let address = Address::from_string(&address_str)
        .map_err(|e| CommandError::InvalidArgument(format!("Invalid address: {}", e)))?;

    // Build the transfer
    let transfer = TransferBuilder {
        destination: address.clone(),
        amount,
        asset: asset.clone(),
        extra_data: None,
    };
    let tx_type = TransactionTypeBuilder::Transfers(vec![transfer]);
    // Use a higher fee multiplier to ensure the transaction has sufficient fees
    // Default fee might be too low for multisig transactions
    let fee = FeeBuilder::Multiplier(2.0);

    // Create unsigned transaction
    let storage = wallet.get_storage().write().await;
    let mut state = wallet
        .create_transaction_state_with_storage(&storage, &tx_type, &fee, None)
        .await
        .context("Error while creating transaction state")?;

    let unsigned = wallet
        .create_unsigned_transaction(
            &mut state,
            None, // No multisig threshold specified - will be set when signing
            tx_type,
            fee,
            storage.get_tx_version().await?,
        )
        .context("Error while building unsigned transaction")?;

    // Get the hash for multisig signing
    let tx_hash = unsigned.get_hash_for_multisig();

    // Serialize the unsigned transaction to hex
    let tx_data = unsigned.to_hex();

    let wallet_address = wallet.get_address();
    manager.message("Unsigned transaction created for multisig signing:");
    manager.message(format!("source: {}", wallet_address));
    manager.message(format!("tx_hash: {}", tx_hash));
    manager.message(format!("tx_data: {}", tx_data));
    manager.message(format!("Recipient: {}", address));
    manager.message(format!(
        "Amount: {} {}",
        format_coin(amount, 8),
        if asset == Hash::zero() {
            "TOS"
        } else {
            "asset"
        }
    ));
    manager.message("");
    manager.message("To sign with each participant wallet:");
    manager.message(format!(
        "  multisig_sign tx_hash={} source={}",
        tx_hash, wallet_address
    ));
    manager.message("");
    manager.message("To submit with all signatures collected:");
    manager.message(format!(
        "  multisig_sign tx_hash={} source={} tx_data={} signatures=<id:sig,...> submit=true",
        tx_hash, wallet_address, tx_data
    ));

    Ok(())
}

// broadcast tx if possible
// In stateless mode, nonce is always queried from daemon before building tx
async fn broadcast_tx(wallet: &Wallet, manager: &CommandManager, tx: Transaction) {
    let tx_hash = tx.hash();
    manager.message(format!("Transaction hash: {}", tx_hash));

    // Stateless wallet: Check if we have daemon connection (network_handler exists)
    let has_connection = wallet.get_network_handler().lock().await.is_some();
    if has_connection {
        if let Err(e) = wallet.submit_transaction(&tx).await {
            manager.error(format!("Couldn't submit transaction: {:#}", e));
            manager.error("Transaction failed. Check your connection to the daemon and try again.");
            // Stateless mode: no local cache to clear - nonce is queried fresh from daemon
        } else {
            manager.message("Transaction submitted successfully!");
        }
    } else {
        manager.warn("You are currently offline, transaction cannot be send automatically. Please send it manually to the network.");
        manager.message(format!("Transaction in hex format: {}", tx.to_hex()));
    }
}

/// Freeze TOS to get energy with duration-based rewards
async fn freeze_tos(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("freeze_tos", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Get amount and duration from arguments (batch mode only)
    let amount_str = if args.has_argument("amount") {
        args.get_value("amount")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument("amount".to_string()));
    };

    let duration_num = if args.has_argument("duration") {
        args.get_value("duration")?.to_number()?
    } else {
        return Err(CommandError::MissingArgument("duration".to_string()));
    };

    // Parse amount
    let amount = from_coin(&amount_str, 8).context("Invalid amount")?;

    // Parse duration (3-365 days)
    let duration = if (3..=365).contains(&duration_num) {
        tos_common::account::FreezeDuration::new(duration_num as u32)
            .map_err(|e| CommandError::InvalidArgument(e.to_string()))?
    } else {
        return Err(CommandError::InvalidArgument(
            "Duration must be between 3 and 365 days".to_string(),
        ));
    };

    // Create freeze transaction
    let _payload = tos_common::transaction::EnergyPayload::FreezeTos { amount, duration };

    manager.message("Building transaction...");

    // Create energy transaction builder with validated parameters
    let energy_builder = EnergyBuilder::freeze_tos(amount, duration);

    // Validate the builder configuration before creating transaction
    if let Err(e) = energy_builder.validate() {
        manager.error(format!("Invalid energy builder configuration: {}", e));
        return Ok(());
    }

    let tx_type = TransactionTypeBuilder::Energy(energy_builder);
    let fee = FeeBuilder::default();

    let tx = match wallet.create_transaction(tx_type, fee).await {
        Ok(tx) => tx,
        Err(e) => {
            manager.error(format!("Error while creating transaction: {}", e));
            return Ok(());
        }
    };

    let hash = tx.hash();
    manager.message(format!("Freeze transaction created: {}", hash));
    manager.message(format!("Amount: {} TOS", format_coin(amount, 8)));
    manager.message(format!("Duration: {:?}", duration));
    manager.message(format!(
        "Reward multiplier: {}x",
        duration.reward_multiplier()
    ));

    // Note: Energy state is now tracked by daemon, not local storage
    // The actual energy gained will be calculated by daemon when the transaction is confirmed
    manager.message("Note: Energy will be credited after transaction confirmation.");

    // Broadcast the transaction
    broadcast_tx(wallet, manager, tx).await;

    Ok(())
}

/// Freeze TOS and delegate energy to other accounts
async fn freeze_tos_delegate(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("freeze_tos_delegate", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let duration_num = if args.has_argument("duration") {
        args.get_value("duration")?.to_number()?
    } else {
        return Err(CommandError::MissingArgument("duration".to_string()));
    };

    let delegatees_str = if args.has_argument("delegatees") {
        args.get_value("delegatees")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument("delegatees".to_string()));
    };

    let duration = tos_common::account::FreezeDuration::new(duration_num as u32)
        .map_err(|e| CommandError::InvalidArgument(e.to_string()))?;

    let mut delegatees = Vec::new();
    for entry in delegatees_str.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let mut parts = entry.split(':');
        let addr_str = parts.next().ok_or_else(|| {
            CommandError::InvalidArgument("Invalid delegatees format".to_string())
        })?;
        let amount_str = parts.next().ok_or_else(|| {
            CommandError::InvalidArgument("Invalid delegatees format".to_string())
        })?;
        if parts.next().is_some() {
            return Err(CommandError::InvalidArgument(
                "Invalid delegatees format".to_string(),
            ));
        }

        let address: tos_common::crypto::Address = addr_str.parse().map_err(|e| {
            CommandError::InvalidArgument(format!("Invalid delegatee address: {e}"))
        })?;
        if address.is_mainnet() != wallet.get_network().is_mainnet() {
            return Err(CommandError::InvalidArgument(
                "Delegatee address network does not match wallet network".to_string(),
            ));
        }
        let amount = from_coin(amount_str, 8).context("Invalid delegatee amount")?;

        delegatees.push(DelegationEntry {
            delegatee: address.get_public_key().clone(),
            amount,
        });
    }

    if delegatees.is_empty() {
        return Err(CommandError::InvalidArgument(
            "Delegatees list cannot be empty".to_string(),
        ));
    }

    let energy_builder = EnergyBuilder::freeze_tos_delegate(delegatees, duration)
        .map_err(|e| CommandError::InvalidArgument(e.to_string()))?;

    if let Err(e) = energy_builder.validate() {
        manager.error(format!("Invalid energy builder configuration: {}", e));
        return Ok(());
    }

    let tx_type = TransactionTypeBuilder::Energy(energy_builder);
    let fee = FeeBuilder::default();

    let tx = match wallet.create_transaction(tx_type, fee).await {
        Ok(tx) => tx,
        Err(e) => {
            manager.error(format!("Error while creating transaction: {}", e));
            return Ok(());
        }
    };

    let hash = tx.hash();
    manager.message(format!("Delegation freeze transaction created: {}", hash));
    manager.message(format!("Duration: {:?}", duration));
    manager.message(format!(
        "Delegatees: {} accounts",
        delegatees_str
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .count()
    ));

    broadcast_tx(wallet, manager, tx).await;

    Ok(())
}

/// Unfreeze TOS (release frozen TOS after lock period)
async fn unfreeze_tos(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("unfreeze_tos", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Get amount from arguments (batch mode only)
    let amount_str = if args.has_argument("amount") {
        args.get_value("amount")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument("amount".to_string()));
    };

    let amount = from_coin(&amount_str, 8).context("Invalid amount")?;

    // Create unfreeze transaction
    let _payload = tos_common::transaction::EnergyPayload::UnfreezeTos {
        amount,
        from_delegation: false,
        record_index: None,      // Use FIFO mode by default
        delegatee_address: None, // No specific delegatee
    };

    manager.message("Building transaction...");

    // Create energy transaction builder with validated parameters
    let energy_builder = EnergyBuilder::unfreeze_tos(amount);

    // Validate the builder configuration before creating transaction
    if let Err(e) = energy_builder.validate() {
        manager.error(format!("Invalid energy builder configuration: {}", e));
        return Ok(());
    }

    let tx_type = TransactionTypeBuilder::Energy(energy_builder);
    let fee = FeeBuilder::default();

    manager.message("Building transaction...");
    let tx = match wallet.create_transaction(tx_type, fee).await {
        Ok(tx) => tx,
        Err(e) => {
            manager.error(format!("Error while creating transaction: {}", e));
            return Ok(());
        }
    };

    let hash = tx.hash();
    manager.message(format!("Unfreeze transaction created: {}", hash));
    manager.message(format!("Amount: {} TOS", format_coin(amount, 8)));

    // Note: Energy state is now tracked by daemon, not local storage
    // The actual energy removed will be calculated by daemon when the transaction is confirmed
    manager.message("Note: Energy will be updated after transaction confirmation.");

    // Broadcast the transaction
    broadcast_tx(wallet, manager, tx).await;

    Ok(())
}

/// Unfreeze delegated TOS (delegation revoke)
async fn unfreeze_tos_delegate(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("unfreeze_tos_delegate", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let amount_str = if args.has_argument("amount") {
        args.get_value("amount")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument("amount".to_string()));
    };

    let amount = from_coin(&amount_str, 8).context("Invalid amount")?;

    let delegatee = if args.has_argument("delegatee") {
        let delegatee_str = args.get_value("delegatee")?.to_string_value()?;
        let delegatee_addr: tos_common::crypto::Address = delegatee_str.parse().map_err(|e| {
            CommandError::InvalidArgument(format!("Invalid delegatee address: {e}"))
        })?;
        if delegatee_addr.is_mainnet() != wallet.get_network().is_mainnet() {
            return Err(CommandError::InvalidArgument(
                "Delegatee address network does not match wallet network".to_string(),
            ));
        }
        Some(delegatee_addr.get_public_key().clone())
    } else {
        None
    };

    let record_index = if args.has_argument("record_index") {
        Some(args.get_value("record_index")?.to_number()? as u32)
    } else {
        None
    };

    let energy_builder = EnergyBuilder::unfreeze_tos_delegated(amount, record_index, delegatee);

    if let Err(e) = energy_builder.validate() {
        manager.error(format!("Invalid energy builder configuration: {}", e));
        return Ok(());
    }

    // Save delegatee address before moving energy_builder
    let delegatee_for_display = energy_builder.delegatee_address.clone();

    let tx_type = TransactionTypeBuilder::Energy(energy_builder);
    let fee = FeeBuilder::default();

    let tx = match wallet.create_transaction(tx_type, fee).await {
        Ok(tx) => tx,
        Err(e) => {
            manager.error(format!("Error while creating transaction: {}", e));
            return Ok(());
        }
    };

    let hash = tx.hash();
    manager.message(format!("Delegation unfreeze transaction created: {}", hash));
    manager.message(format!("Amount: {} TOS", format_coin(amount, 8)));
    if let Some(delegatee_pub) = delegatee_for_display {
        manager.message(format!(
            "Delegatee: {}",
            delegatee_pub.to_address(wallet.get_network().is_mainnet())
        ));
    }

    broadcast_tx(wallet, manager, tx).await;

    Ok(())
}

/// Withdraw all expired pending unfreezes
async fn withdraw_unfrozen(
    manager: &CommandManager,
    _args: ArgumentManager,
) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let energy_builder = EnergyBuilder::withdraw_unfrozen();

    if let Err(e) = energy_builder.validate() {
        manager.error(format!("Invalid energy builder configuration: {}", e));
        return Ok(());
    }

    let tx_type = TransactionTypeBuilder::Energy(energy_builder);
    let fee = FeeBuilder::default();

    let tx = match wallet.create_transaction(tx_type, fee).await {
        Ok(tx) => tx,
        Err(e) => {
            manager.error(format!("Error while creating transaction: {}", e));
            return Ok(());
        }
    };

    let hash = tx.hash();
    manager.message(format!("Withdraw transaction created: {}", hash));

    broadcast_tx(wallet, manager, tx).await;

    Ok(())
}

/// Show energy information and freeze records
async fn energy_info(manager: &CommandManager, _args: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Query energy info from daemon API (stateless wallet)
    let network_handler = wallet.get_network_handler().lock().await;
    let handler = network_handler.as_ref().ok_or_else(|| {
        CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
    })?;

    {
        let api = handler.get_api();
        let address = wallet.get_address();

        match api
            .call(
                &"get_energy".to_string(),
                &tos_common::api::daemon::GetEnergyParams {
                    address: Cow::Borrowed(&address),
                },
            )
            .await
        {
            Ok(result) => {
                let energy_result: tos_common::api::daemon::GetEnergyResult =
                    serde_json::from_value(result).context("Failed to parse energy result")?;

                manager.message(format!("Energy Information for {}:", address));
                manager.message(format!(
                    "  Frozen TOS: {} TOS",
                    format_tos(energy_result.frozen_tos)
                ));
                manager.message(format!("  Energy: {} units", energy_result.energy));
                manager.message(format!(
                    "  Available Energy: {} units",
                    energy_result.available_energy
                ));
                manager.message(format!(
                    "  Last Update: topoheight {}",
                    energy_result.last_update
                ));

                if !energy_result.freeze_records.is_empty() {
                    manager.message("  Freeze Records:");
                    for (i, record) in energy_result.freeze_records.iter().enumerate() {
                        manager.message(format!(
                            "    Record {}: {} TOS for {} days",
                            i + 1,
                            format_tos(record.amount),
                            record.duration
                        ));
                        manager.message(format!(
                            "      Energy Gained: {} units",
                            record.energy_gained
                        ));
                        manager.message(format!(
                            "      Freeze Time: topoheight {}",
                            record.freeze_topoheight
                        ));
                        manager.message(format!(
                            "      Unlock Time: topoheight {}",
                            record.unlock_topoheight
                        ));

                        if record.can_unlock {
                            manager.message("      Status: ✅ Unlockable".to_string());
                        } else {
                            // Use network-specific blocks per day for accurate display
                            // Mainnet/Testnet: 86400 blocks/day, Devnet: 10 blocks/day
                            let blocks_per_day =
                                wallet.get_network().freeze_duration_multiplier() as f64;
                            let remaining_days = record.remaining_blocks as f64 / blocks_per_day;
                            if wallet.get_network().is_devnet() {
                                // Devnet: show in blocks for clarity
                                manager.message(format!(
                                    "      Status: 🔒 Locked ({} blocks remaining, ~{:.1} devnet-days)",
                                    record.remaining_blocks, remaining_days
                                ));
                            } else {
                                manager.message(format!(
                                    "      Status: 🔒 Locked ({:.2} days remaining)",
                                    remaining_days
                                ));
                            }
                        }
                    }
                }

                if !energy_result.delegated_records.is_empty() {
                    manager.message("  Delegated Freeze Records:");
                    for (i, record) in energy_result.delegated_records.iter().enumerate() {
                        manager.message(format!(
                            "    Record {}: {} TOS for {} days",
                            i + 1,
                            format_tos(record.total_amount),
                            record.duration
                        ));
                        manager
                            .message(format!("      Total Energy: {} units", record.total_energy));
                        manager.message(format!(
                            "      Freeze Time: topoheight {}",
                            record.freeze_topoheight
                        ));
                        manager.message(format!(
                            "      Unlock Time: topoheight {}",
                            record.unlock_topoheight
                        ));

                        if record.can_unlock {
                            manager.message("      Status: ✅ Unlockable".to_string());
                        } else {
                            let blocks_per_day =
                                wallet.get_network().freeze_duration_multiplier() as f64;
                            let remaining_days = record.remaining_blocks as f64 / blocks_per_day;
                            if wallet.get_network().is_devnet() {
                                manager.message(format!(
                                    "      Status: 🔒 Locked ({} blocks remaining, ~{:.1} devnet-days)",
                                    record.remaining_blocks, remaining_days
                                ));
                            } else {
                                manager.message(format!(
                                    "      Status: 🔒 Locked ({:.2} days remaining)",
                                    remaining_days
                                ));
                            }
                        }

                        if !record.entries.is_empty() {
                            manager.message("      Delegatees:");
                            for entry in &record.entries {
                                manager.message(format!(
                                    "        {}: {} TOS, {} energy",
                                    entry.delegatee,
                                    format_tos(entry.amount),
                                    entry.energy
                                ));
                            }
                        }
                    }
                }

                if !energy_result.pending_unfreezes.is_empty() {
                    manager.message("  Pending Unfreezes:");
                    for (i, pending) in energy_result.pending_unfreezes.iter().enumerate() {
                        manager.message(format!(
                            "    Pending {}: {} TOS",
                            i + 1,
                            format_tos(pending.amount)
                        ));
                        manager.message(format!(
                            "      Expire Time: topoheight {}",
                            pending.expire_topoheight
                        ));
                        if pending.can_withdraw {
                            manager.message("      Status: ✅ Withdrawable".to_string());
                        } else {
                            manager.message(format!(
                                "      Status: 🕒 Cooling ({} blocks remaining)",
                                pending.remaining_blocks
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                manager.error(format!("Failed to get energy information: {}", e));
            }
        }
    }

    Ok(())
}

/// Bind a referrer to the sender account (one-time, immutable)
async fn bind_referrer(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("bind_referrer", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Get referrer address from arguments
    let referrer_str = if args.has_argument("referrer") {
        args.get_value("referrer")?.to_string_value()?
    } else {
        return Err(CommandError::MissingArgument("referrer".to_string()));
    };

    // Parse the referrer address
    let referrer_address: tos_common::crypto::Address = referrer_str
        .parse()
        .map_err(|e| CommandError::InvalidArgument(format!("Invalid referrer address: {}", e)))?;

    // Validate network matches
    if referrer_address.is_mainnet() != wallet.get_network().is_mainnet() {
        return Err(CommandError::InvalidArgument(
            "Referrer address network does not match wallet network".to_string(),
        ));
    }

    // Cannot set self as referrer
    if referrer_address == wallet.get_address() {
        return Err(CommandError::InvalidArgument(
            "Cannot set yourself as referrer".to_string(),
        ));
    }

    manager.message(format!("Binding referrer: {}", referrer_address));

    // Create BindReferrer payload
    let payload = tos_common::transaction::BindReferrerPayload::new(
        referrer_address.get_public_key().clone(),
        None, // No extra data for now
    );

    let tx_type = tos_common::transaction::builder::TransactionTypeBuilder::BindReferrer(payload);
    let fee = tos_common::transaction::builder::FeeBuilder::default();

    manager.message("Building transaction...");
    let tx = match wallet.create_transaction(tx_type, fee).await {
        Ok(tx) => tx,
        Err(e) => {
            manager.error(format!("Error while creating transaction: {}", e));
            return Ok(());
        }
    };

    let hash = tx.hash();
    manager.message(format!("Bind referrer transaction created: {}", hash));
    manager.message(format!("Referrer: {}", referrer_address));
    manager.message("Note: This is a one-time operation. Once bound, it cannot be changed.");

    // Broadcast the transaction
    broadcast_tx(wallet, manager, tx).await;

    Ok(())
}

/// Show referral information for current wallet
async fn referral_info(
    manager: &CommandManager,
    _args: ArgumentManager,
) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let network_handler = wallet.get_network_handler().lock().await;
    let handler = network_handler.as_ref().ok_or_else(|| {
        CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
    })?;

    let daemon_api = handler.get_api();
    let address = wallet.get_address();

    manager.message(format!("Referral Information for {}:", address));
    manager.message("");

    // Check if user has a referrer
    match daemon_api.has_referrer(&address).await {
        Ok(has_referrer) => {
            if has_referrer {
                // Get referrer details
                match daemon_api.get_referrer(&address).await {
                    Ok(Some(referrer)) => {
                        manager.message(format!("  Referrer: {}", referrer));
                    }
                    Ok(None) => {
                        manager.message("  Referrer: None");
                    }
                    Err(e) => {
                        manager.error(format!("  Failed to get referrer: {}", e));
                    }
                }

                // Get referral record for more details
                match daemon_api.get_referral_record(&address).await {
                    Ok(record) => {
                        manager.message(format!(
                            "  Bound at topoheight: {}",
                            record.bound_at_topoheight
                        ));
                        manager.message(format!("  Bound tx: {}", record.bound_tx_hash));
                        manager.message(format!(
                            "  Direct referrals count: {}",
                            record.direct_referrals_count
                        ));
                        manager.message(format!("  Team size: {}", record.team_size));
                    }
                    Err(e) => {
                        manager.error(format!("  Failed to get referral record: {}", e));
                    }
                }
            } else {
                manager.message("  Referrer: Not bound");
                manager.message("  Use 'bind_referrer <address>' to bind a referrer.");
            }
        }
        Err(e) => {
            manager.error(format!("Failed to check referrer status: {}", e));
        }
    }

    // Get referral level
    match daemon_api.get_referral_level(&address).await {
        Ok(level) => {
            manager.message(format!("  Referral level: {}", level));
        }
        Err(_) => {
            // Silently ignore - may not have a referrer
        }
    }

    // Get team size
    match daemon_api.get_team_size(&address, true).await {
        Ok(result) => {
            manager.message(format!(
                "  Team size: {} (from cache: {})",
                result.team_size, result.from_cache
            ));
        }
        Err(_) => {
            // Silently ignore
        }
    }

    // Get direct referrals count
    match daemon_api.get_direct_referrals(&address, 0, 1).await {
        Ok(result) => {
            manager.message(format!("  Direct referrals: {}", result.total_count));
        }
        Err(_) => {
            // Silently ignore
        }
    }

    Ok(())
}

/// Get upline chain for current wallet
async fn get_uplines(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Get levels from arguments (default 10)
    let levels = if args.has_argument("levels") {
        let n = args.get_value("levels")?.to_number()?;
        if n > 20 {
            return Err(CommandError::InvalidArgument(
                "Maximum levels is 20".to_string(),
            ));
        }
        n as u8
    } else {
        10
    };

    let network_handler = wallet.get_network_handler().lock().await;
    let handler = network_handler.as_ref().ok_or_else(|| {
        CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
    })?;

    let daemon_api = handler.get_api();
    let address = wallet.get_address();

    match daemon_api.get_uplines(&address, levels).await {
        Ok(result) => {
            if result.uplines.is_empty() {
                manager.message("No uplines found. You may not have bound a referrer yet.");
            } else {
                manager.message(format!("Upline chain ({} levels):", result.levels_returned));
                for (i, upline) in result.uplines.iter().enumerate() {
                    manager.message(format!("  Level {}: {}", i + 1, upline));
                }
            }
        }
        Err(e) => {
            manager.error(format!("Failed to get uplines: {}", e));
        }
    }

    Ok(())
}

/// Get direct referrals for current wallet
async fn get_direct_referrals(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Get pagination parameters
    let offset = if args.has_argument("offset") {
        args.get_value("offset")?.to_number()? as u32
    } else {
        0
    };

    let limit = if args.has_argument("limit") {
        let n = args.get_value("limit")?.to_number()?;
        if n > 100 {
            return Err(CommandError::InvalidArgument(
                "Maximum limit is 100".to_string(),
            ));
        }
        n as u32
    } else {
        20
    };

    let network_handler = wallet.get_network_handler().lock().await;
    let handler = network_handler.as_ref().ok_or_else(|| {
        CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
    })?;

    let daemon_api = handler.get_api();
    let address = wallet.get_address();

    match daemon_api
        .get_direct_referrals(&address, offset, limit)
        .await
    {
        Ok(result) => {
            manager.message(format!(
                "Direct referrals: {} total (showing {}-{})",
                result.total_count,
                offset + 1,
                offset + result.referrals.len() as u32
            ));

            if result.referrals.is_empty() {
                manager.message("  No direct referrals found.");
            } else {
                for (i, referral) in result.referrals.iter().enumerate() {
                    manager.message(format!("  {}. {}", offset + i as u32 + 1, referral));
                }
            }

            if result.has_more {
                manager.message(format!(
                    "\nMore results available. Use 'get_direct_referrals {} {}' to see next page.",
                    offset + limit,
                    limit
                ));
            }
        }
        Err(e) => {
            manager.error(format!("Failed to get direct referrals: {}", e));
        }
    }

    Ok(())
}

/// Execute JSON batch command
async fn execute_json_batch(
    command_manager: &CommandManager,
    json_content: &str,
    config: &Config,
) -> Result<(), anyhow::Error> {
    // Parse JSON
    let json_config: JsonBatchConfig = serde_json::from_str(json_content)
        .with_context(|| "Failed to parse JSON batch configuration")?;

    if log::log_enabled!(log::Level::Info) {
        info!("Executing JSON batch command: {}", json_config.command);
    }

    // Override wallet_path and password from JSON if provided
    // but CLI parameters take precedence
    let _wallet_path = config
        .wallet_path
        .as_ref()
        .or(json_config.wallet_path.as_ref())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No wallet path specified. Use --wallet-path or provide wallet_path in JSON"
            )
        })?;

    let _password = config
        .password
        .as_ref()
        .or(json_config.password.as_ref())
        .ok_or_else(|| {
            anyhow::anyhow!("No password specified. Use --password or provide password in JSON")
        })?;

    // Store wallet info in command manager context if needed
    // This would require additional setup for wallet loading in JSON mode
    // For now, we assume the wallet is already loaded

    match command_manager
        .handle_json_command(&json_config.command, &json_config.params)
        .await
    {
        Ok(_) => {
            if log::log_enabled!(log::Level::Info) {
                info!("JSON batch command executed successfully");
            }
            Ok(())
        }
        Err(e) => {
            if log::log_enabled!(log::Level::Error) {
                error!("Error executing JSON batch command: {:#}", e);
            }
            Err(e.into())
        }
    }
}

// Deploy a smart contract to the blockchain
async fn deploy_contract(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let wallet = {
        let context = manager.get_context().lock()?;
        context.get::<Arc<Wallet>>()?.clone()
    };

    // Wait for wallet to sync before creating transaction

    // Stateless wallet: Just check if daemon is connected
    if !wallet.is_synced().await.unwrap_or(false) {
        manager.error("Wallet is not connected to daemon. Use 'online_mode' command first.");
        return Ok(());
    }

    // Get contract file path (batch mode only)
    let file_path = get_required_arg_with_example(
        &mut args,
        "file",
        "deploy_contract <file>",
        "deploy_contract /path/to/counter.so",
    )
    .context("Error while reading file path")?;

    // Read contract file
    let contract_bytes = match std::fs::read(&file_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            manager.error(format!(
                "Failed to read contract file '{}': {}",
                file_path, e
            ));
            return Ok(());
        }
    };

    // Verify ELF magic bytes
    if contract_bytes.len() < 4 || &contract_bytes[0..4] != b"\x7FELF" {
        manager.error("Invalid contract file: not a valid ELF binary");
        return Ok(());
    }

    let contract_size = contract_bytes.len();
    manager.message(format!(
        "Contract file: {} ({} bytes)",
        file_path, contract_size
    ));

    // Create a TAKO module from the ELF bytecode and serialize it
    // Clone the bytes since we'll need the original for computing the contract address later
    let module = Module::from_bytecode(contract_bytes.clone());
    let module_bytes = module.to_bytes();
    let module_hex = hex::encode(&module_bytes);

    manager.message("Building deployment transaction...");

    // Create deploy contract transaction
    let deploy_builder = DeployContractBuilder {
        module: module_hex,
        invoke: None,
    };

    let tx_type = TransactionTypeBuilder::DeployContract(deploy_builder);
    let fee = FeeBuilder::default();

    let tx = match wallet.create_transaction(tx_type, fee).await {
        Ok(tx) => tx,
        Err(e) => {
            manager.error(format!("Error while creating transaction: {e}"));
            return Ok(());
        }
    };

    let hash = tx.hash();
    manager.message(format!("Contract deployment transaction created: {hash}"));
    manager.message(format!("Contract size: {} bytes", contract_size));

    // Compute the deterministic contract address
    // The address is computed as: blake3(0xff || deployer_pubkey || blake3(bytecode))
    let contract_address = tos_common::crypto::compute_deterministic_contract_address(
        tx.get_source(),
        &contract_bytes,
    );
    manager.message(format!("Contract address: {}", contract_address));
    manager.message("");
    manager.message("IMPORTANT: Use the Contract Address (not the TX hash) for:");
    manager.message("  - invoke_contract <contract_address> <entry_id>");
    manager.message("  - get_contract_info <contract_address>");
    manager.message("  - get_contract_balance <contract_address> <asset>");

    // Broadcast the transaction
    broadcast_tx(&wallet, manager, tx).await;

    Ok(())
}

// Invoke a smart contract function
async fn invoke_contract(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let wallet = {
        let context = manager.get_context().lock()?;
        context.get::<Arc<Wallet>>()?.clone()
    };

    // Wait for wallet to sync before creating transaction

    // Stateless wallet: Just check if daemon is connected
    if !wallet.is_synced().await.unwrap_or(false) {
        manager.error("Wallet is not connected to daemon. Use 'online_mode' command first.");
        return Ok(());
    }

    // Get contract address (deployment TX hash)
    let contract_str = get_required_arg_with_example(
        &mut args,
        "contract",
        "invoke_contract <contract> <entry_id> [max_gas]",
        "invoke_contract abc123...def 0",
    )
    .context("Error while reading contract address")?;

    let contract = match Hash::from_hex(&contract_str) {
        Ok(h) => h,
        Err(e) => {
            manager.error(format!("Invalid contract hash: {e}"));
            return Ok(());
        }
    };

    // Get entry point ID
    let entry_id_str = get_required_arg_with_example(
        &mut args,
        "entry_id",
        "invoke_contract <contract> <entry_id> [max_gas]",
        "invoke_contract abc123...def 0",
    )
    .context("Error while reading entry ID")?;

    let entry_id: u16 = match entry_id_str.parse() {
        Ok(id) => id,
        Err(e) => {
            manager.error(format!("Invalid entry ID: {e}"));
            return Ok(());
        }
    };

    // Optional data parameter (hex-encoded bytes)
    use tos_common::contract::ValueCell;
    let parameters: Vec<ValueCell> = if args.has_argument("data") {
        let data_hex = args.get_value("data")?.to_string_value()?;
        match hex::decode(&data_hex) {
            Ok(bytes) => {
                manager.message(format!("Call data: {} bytes", bytes.len()));
                vec![ValueCell::Bytes(bytes)]
            }
            Err(e) => {
                manager.error(format!("Invalid hex data: {e}"));
                return Ok(());
            }
        }
    } else {
        vec![]
    };

    // Optional max_gas parameter (default: 1_000_000)
    let max_gas: u64 = if args.has_argument("max_gas") {
        args.get_value("max_gas")?
            .to_string_value()?
            .parse()
            .unwrap_or(1_000_000)
    } else {
        1_000_000
    };

    // Optional deposit parameter (amount in atomic units to deposit to contract)
    let deposit_amount: u64 = if args.has_argument("deposit") {
        args.get_value("deposit")?
            .to_string_value()?
            .parse()
            .unwrap_or(0)
    } else {
        0
    };

    manager.message(format!(
        "Invoking contract {} with entry_id={}, max_gas={}, params={}",
        contract,
        entry_id,
        max_gas,
        parameters.len()
    ));

    // Build deposits IndexMap
    use indexmap::IndexMap;
    let mut deposits: IndexMap<Hash, ContractDepositBuilder> = IndexMap::new();
    if deposit_amount > 0 {
        manager.message(format!(
            "Depositing {} TOS to contract",
            format_tos(deposit_amount)
        ));
        deposits.insert(
            TOS_ASSET,
            ContractDepositBuilder {
                amount: deposit_amount,
                private: false,
            },
        );
    }

    // Create invoke contract transaction
    let invoke_builder = InvokeContractBuilder {
        contract,
        max_gas,
        entry_id,
        parameters,
        deposits,
        contract_key: None,
    };

    let tx_type = TransactionTypeBuilder::InvokeContract(invoke_builder);
    let fee = FeeBuilder::default();

    let tx = match wallet.create_transaction(tx_type, fee).await {
        Ok(tx) => tx,
        Err(e) => {
            manager.error(format!("Error while creating transaction: {e}"));
            return Ok(());
        }
    };

    let hash = tx.hash();
    manager.message(format!("Contract invocation transaction created: {hash}"));

    // Broadcast the transaction
    broadcast_tx(&wallet, manager, tx).await;

    Ok(())
}

// Get information about a deployed contract
async fn get_contract_info(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let wallet = {
        let context = manager.get_context().lock()?;
        context.get::<Arc<Wallet>>()?.clone()
    };

    // Get contract address (deployment TX hash)
    let contract_str = get_required_arg_with_example(
        &mut args,
        "contract",
        "get_contract_info <contract>",
        "get_contract_info abc123...def",
    )
    .context("Error while reading contract address")?;

    let contract = match Hash::from_hex(&contract_str) {
        Ok(h) => h,
        Err(e) => {
            manager.error(format!("Invalid contract hash: {e}"));
            return Ok(());
        }
    };

    manager.message(format!("Querying contract {}...", contract));

    // Get contract module from daemon

    {
        let network_handler = wallet.get_network_handler().lock().await;
        if let Some(handler) = network_handler.as_ref() {
            let api = handler.get_api();
            match api.get_contract_module(&contract).await {
                Ok(module) => {
                    let bytecode_size = module.get_bytecode().map(|b| b.len()).unwrap_or(0);
                    manager.message(format!("Contract: {}", contract));
                    manager.message(format!("Bytecode size: {} bytes", bytecode_size));
                    manager.message("Contract exists and is deployed.");
                }
                Err(e) => {
                    manager.error(format!("Failed to get contract info: {e}"));
                }
            }
        } else {
            manager.error("Not connected to daemon. Ensure daemon is running and accessible.");
        }
    }

    Ok(())
}

// Get contract address from a deployment transaction hash
async fn get_contract_address(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let wallet = {
        let context = manager.get_context().lock()?;
        context.get::<Arc<Wallet>>()?.clone()
    };

    // Get deployment TX hash
    let tx_hash_str = get_required_arg_with_example(
        &mut args,
        "tx_hash",
        "get_contract_address <tx_hash>",
        "get_contract_address abc123...def",
    )
    .context("Error while reading transaction hash")?;

    let tx_hash = match Hash::from_hex(&tx_hash_str) {
        Ok(h) => h,
        Err(e) => {
            manager.error(format!("Invalid transaction hash: {e}"));
            return Ok(());
        }
    };

    manager.message(format!(
        "Looking up contract address from TX {}...",
        tx_hash
    ));

    // Get contract address from daemon

    {
        let network_handler = wallet.get_network_handler().lock().await;
        if let Some(handler) = network_handler.as_ref() {
            let api = handler.get_api();
            match api.get_contract_address_from_tx(&tx_hash).await {
                Ok(result) => {
                    manager.message(format!("Deployment TX: {}", tx_hash));
                    manager.message(format!("Contract Address: {}", result.contract_address));
                    manager.message(format!("Deployer: {}", result.deployer));
                    manager.message("");
                    manager.message("NOTE: Use the Contract Address (not the TX hash) for:");
                    manager.message("  - invoke_contract <contract_address> <entry_id>");
                    manager.message("  - get_contract_info <contract_address>");
                    manager.message("  - get_contract_balance <contract_address> <asset>");
                }
                Err(e) => {
                    manager.error(format!("Failed to get contract address: {e}"));
                    manager.message(
                        "Make sure the transaction hash is from a DeployContract transaction.",
                    );
                }
            }
        } else {
            manager.error("Not connected to daemon. Ensure daemon is running and accessible.");
        }
    }

    Ok(())
}

// Get the balance of a contract for a specific asset
async fn get_contract_balance(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let wallet = {
        let context = manager.get_context().lock()?;
        context.get::<Arc<Wallet>>()?.clone()
    };

    // Get contract address (deployment TX hash)
    let contract_str = get_required_arg_with_example(
        &mut args,
        "contract",
        "get_contract_balance <contract> <asset>",
        "get_contract_balance abc123...def 0",
    )
    .context("Error while reading contract address")?;

    let contract = match Hash::from_hex(&contract_str) {
        Ok(h) => h,
        Err(e) => {
            manager.error(format!("Invalid contract hash: {e}"));
            return Ok(());
        }
    };

    // Get asset hash (use TOS_ASSET for native TOS)
    let asset_str = get_required_arg_with_example(
        &mut args,
        "asset",
        "get_contract_balance <contract> <asset>",
        "get_contract_balance abc123...def 0",
    )
    .context("Error while reading asset hash")?;

    let asset = if asset_str == "0" {
        TOS_ASSET
    } else {
        match Hash::from_hex(&asset_str) {
            Ok(h) => h,
            Err(e) => {
                manager.error(format!("Invalid asset hash: {e}"));
                return Ok(());
            }
        }
    };

    manager.message(format!("Querying balance for contract {}...", contract));

    // Get contract balance from daemon

    {
        let network_handler = wallet.get_network_handler().lock().await;
        if let Some(handler) = network_handler.as_ref() {
            let api = handler.get_api();
            match api.get_contract_balance(&contract, &asset).await {
                Ok(balance) => {
                    let formatted = format_coin(balance, tos_common::config::COIN_DECIMALS);
                    manager.message(format!("Contract: {}", contract));
                    manager.message(format!("Asset: {}", asset));
                    manager.message(format!("Balance: {} ({} atomic units)", formatted, balance));
                }
                Err(e) => {
                    manager.error(format!("Failed to get contract balance: {e}"));
                }
            }
        } else {
            manager.error("Not connected to daemon. Ensure daemon is running and accessible.");
        }
    }

    Ok(())
}

// Get the total number of deployed contracts
async fn count_contracts(
    manager: &CommandManager,
    _args: ArgumentManager,
) -> Result<(), CommandError> {
    let wallet = {
        let context = manager.get_context().lock()?;
        context.get::<Arc<Wallet>>()?.clone()
    };

    manager.message("Querying contract count...");

    // Get contract count from daemon

    {
        let network_handler = wallet.get_network_handler().lock().await;
        if let Some(handler) = network_handler.as_ref() {
            let api = handler.get_api();
            match api.count_contracts().await {
                Ok(count) => {
                    manager.message(format!("Total deployed contracts: {}", count));
                }
                Err(e) => {
                    manager.error(format!("Failed to get contract count: {e}"));
                }
            }
        } else {
            manager.error("Not connected to daemon. Ensure daemon is running and accessible.");
        }
    }

    Ok(())
}

// ========== TNS (TOS Name Service) Command Handlers ==========

/// Register a TNS name
async fn register_name(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("register_name", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Get the name to register
    let name = get_required_arg_with_example(
        &mut args,
        "name",
        "register_name name=<name>",
        "register_name name=alice",
    )?;

    // Validate name format locally before making RPC call
    let validation = tos_common::tns::validate_name_format(&name);
    if !validation.valid {
        return Err(CommandError::InvalidArgument(format!(
            "Invalid name '{}': {}",
            name,
            validation
                .error
                .unwrap_or_else(|| "Unknown error".to_string())
        )));
    }
    let normalized_name = validation.normalized.ok_or_else(|| {
        CommandError::InvalidArgument("Validation passed but normalized name is None".to_string())
    })?;

    // Check if the name is available
    {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();

        let availability = daemon_api
            .is_name_available(&normalized_name)
            .await
            .map_err(|e| {
                CommandError::Any(anyhow::anyhow!("Failed to check name availability: {}", e))
            })?;

        if !availability.valid_format {
            return Err(CommandError::InvalidArgument(format!(
                "Invalid name format: {}",
                availability
                    .format_error
                    .unwrap_or_else(|| "Unknown error".to_string())
            )));
        }

        if !availability.available {
            return Err(CommandError::InvalidArgument(format!(
                "Name '{}' is already registered.",
                normalized_name
            )));
        }
    }

    manager.message(format!(
        "Registering name '{}' ({}@tos.network)...",
        normalized_name, normalized_name
    ));

    // Build the transaction
    // Registration requires REGISTRATION_FEE (10 TOS)
    let registration_fee = tos_common::tns::REGISTRATION_FEE;
    let fee_builder = tos_common::transaction::builder::FeeBuilder::Value(registration_fee);

    let payload = tos_common::transaction::RegisterNamePayload::new(normalized_name.clone());
    let tx_type = tos_common::transaction::builder::TransactionTypeBuilder::RegisterName(payload);

    let storage = wallet.get_storage().read().await;
    let mut state = wallet
        .create_transaction_state_with_storage(&storage, &tx_type, &fee_builder, None)
        .await
        .context("Error while creating transaction state")?;

    let tx_version = storage
        .get_tx_version()
        .await
        .context("Error while getting tx version")?;

    let builder = tos_common::transaction::builder::TransactionBuilder::new(
        tx_version,
        wallet.get_network().chain_id() as u8,
        wallet.get_public_key().clone(),
        None,
        tx_type,
        fee_builder,
    );

    let tx = match builder.build(&mut state, wallet.get_keypair()) {
        Ok(tx) => tx,
        Err(e) => {
            manager.error(format!("Error while creating transaction: {}", e));
            return Ok(());
        }
    };

    broadcast_tx(wallet, manager, tx).await;
    manager.message(format!(
        "Successfully submitted name registration for '{}'",
        normalized_name
    ));
    Ok(())
}

/// Resolve a TNS name to an address
async fn resolve_name(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("resolve_name", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Get the name to resolve
    let name = get_required_arg_with_example(
        &mut args,
        "name",
        "resolve_name name=<name>",
        "resolve_name name=alice",
    )?;

    // Strip @tos.network suffix if present
    let name_part = if name.ends_with("@tos.network") {
        &name[..name.len() - 12]
    } else {
        &name
    };

    manager.message(format!("Resolving name '{}'...", name_part));

    {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();

        match daemon_api.resolve_name(name_part).await {
            Ok(result) => {
                if let Some(address) = result.address {
                    // Address implements Display, Cow<T> also implements Display when T does
                    manager.message(format!("{}@tos.network -> {}", name_part, address));
                } else {
                    manager.message(format!("Name '{}' is not registered.", name_part));
                }
            }
            Err(e) => {
                manager.error(format!("Failed to resolve name: {}", e));
            }
        }
    }

    Ok(())
}

/// Send an ephemeral message to a TNS name
async fn send_message(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("send_message", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Get recipient name
    let recipient = get_required_arg_with_example(
        &mut args,
        "recipient",
        "send_message recipient=<name> message=<text>",
        "send_message recipient=bob message=\"Hello Bob!\"",
    )?;

    // Get message content
    let message = get_required_arg_with_example(
        &mut args,
        "message",
        "send_message recipient=<name> message=<text>",
        "send_message recipient=bob message=\"Hello Bob!\"",
    )?;

    // Validate message is not empty
    if message.is_empty() {
        return Err(CommandError::InvalidArgument(
            "Message cannot be empty".to_string(),
        ));
    }

    // Validate message size
    if message.len() > tos_common::tns::MAX_MESSAGE_SIZE {
        return Err(CommandError::InvalidArgument(format!(
            "Message too long ({} bytes). Maximum is {} bytes.",
            message.len(),
            tos_common::tns::MAX_MESSAGE_SIZE
        )));
    }

    // Get optional TTL (default: DEFAULT_TTL = 100 blocks)
    let ttl_blocks = if args.has_argument("ttl") {
        let ttl_raw = args.get_value("ttl")?.to_number()?;
        // Validate TTL is within u32 range (to_number returns u64)
        if ttl_raw > u64::from(u32::MAX) {
            return Err(CommandError::InvalidArgument(
                "TTL value exceeds maximum allowed range".to_string(),
            ));
        }
        let ttl = ttl_raw as u32;
        // Validate TTL range against protocol limits
        if !(tos_common::tns::MIN_TTL..=tos_common::tns::MAX_TTL).contains(&ttl) {
            return Err(CommandError::InvalidArgument(format!(
                "TTL must be between {} and {} blocks. Got: {}",
                tos_common::tns::MIN_TTL,
                tos_common::tns::MAX_TTL,
                ttl
            )));
        }
        ttl
    } else {
        tos_common::tns::DEFAULT_TTL
    };

    // Strip @tos.network suffix if present
    let recipient_name = if recipient.ends_with("@tos.network") {
        &recipient[..recipient.len() - 12]
    } else {
        &recipient
    };

    // Check if sender has a registered name
    let sender_name_hash = {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();

        // Check if sender has registered a name
        let sender_name = daemon_api
            .get_account_name_hash(&wallet.get_address())
            .await
            .map_err(|e| {
                CommandError::Any(anyhow::anyhow!("Failed to get sender's name: {}", e))
            })?;

        sender_name.name_hash.ok_or_else(|| {
            CommandError::InvalidArgument(
                "You must register a TNS name before sending messages. Use 'register_name' first."
                    .to_string(),
            )
        })?
    };

    // Resolve recipient name and get their public key for encryption
    let (recipient_name_hash, recipient_public_key) = {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();

        let result = daemon_api.resolve_name(recipient_name).await.map_err(|e| {
            CommandError::Any(anyhow::anyhow!("Failed to resolve recipient: {}", e))
        })?;

        match result.address {
            Some(addr) => {
                let pk = addr.get_public_key().clone();
                (result.name_hash.into_owned(), pk)
            }
            None => {
                return Err(CommandError::InvalidArgument(format!(
                    "Recipient '{}' is not registered.",
                    recipient_name
                )));
            }
        }
    };

    manager.message(format!(
        "Sending message to {}@tos.network (TTL: {} blocks)...",
        recipient_name, ttl_blocks
    ));

    // Get the next available nonce for this transaction
    // This nonce will be used both as the message_nonce (for replay protection)
    // and as the transaction nonce
    let message_nonce = {
        let light_api = wallet
            .get_light_api()
            .await
            .map_err(|e| CommandError::Any(anyhow::anyhow!("Failed to get light API: {}", e)))?;
        light_api
            .get_next_nonce(&wallet.get_address())
            .await
            .map_err(|e| {
                CommandError::Any(anyhow::anyhow!("Failed to get nonce from daemon: {}", e))
            })?
    };

    // Encrypt message using ECDH with recipient's public key
    use tos_common::crypto::elgamal::{DecryptHandle, PedersenOpening};
    use tos_common::transaction::extra_data::{derive_shared_key_from_opening, PlaintextData};

    // Generate random opening for key derivation
    let opening = PedersenOpening::generate_new();

    // Derive shared key: k = SHA3-256(r * H)
    let shared_key = derive_shared_key_from_opening(&opening);

    // Encrypt message content with ChaCha20
    let encrypted_content = PlaintextData(message.as_bytes().to_vec())
        .encrypt_in_place(&shared_key)
        .0;

    // Create receiver handle: r * Pk_recipient (allows recipient to derive same shared key)
    let recipient_pk = recipient_public_key.decompress().map_err(|_| {
        CommandError::InvalidArgument("Failed to decompress recipient's public key".to_string())
    })?;
    let receiver_handle = DecryptHandle::new(&recipient_pk, &opening).compress();

    // Build the transaction
    // Message fee depends on TTL: BASE_MESSAGE_FEE * (1, 2, or 3) based on duration
    let message_fee = tos_common::tns::calculate_message_fee(ttl_blocks);
    let fee_builder = tos_common::transaction::builder::FeeBuilder::Value(message_fee);

    let payload = tos_common::transaction::EphemeralMessagePayload::new(
        sender_name_hash.into_owned(),
        recipient_name_hash,
        message_nonce,
        ttl_blocks,
        encrypted_content,
        *receiver_handle.as_bytes(),
    );
    let tx_type =
        tos_common::transaction::builder::TransactionTypeBuilder::EphemeralMessage(payload);

    let storage = wallet.get_storage().read().await;
    // Use message_nonce as the transaction nonce to ensure they match
    // This prevents InvalidMessageNonce errors if nonce changes between calls
    let mut state = wallet
        .create_transaction_state_with_storage(
            &storage,
            &tx_type,
            &fee_builder,
            Some(message_nonce),
        )
        .await
        .context("Error while creating transaction state")?;

    let tx_version = storage
        .get_tx_version()
        .await
        .context("Error while getting tx version")?;

    let builder = tos_common::transaction::builder::TransactionBuilder::new(
        tx_version,
        wallet.get_network().chain_id() as u8,
        wallet.get_public_key().clone(),
        None,
        tx_type,
        fee_builder,
    );

    let tx = match builder.build(&mut state, wallet.get_keypair()) {
        Ok(tx) => tx,
        Err(e) => {
            manager.error(format!("Error while creating transaction: {}", e));
            return Ok(());
        }
    };

    broadcast_tx(wallet, manager, tx).await;
    manager.message(format!(
        "Successfully sent message to {}@tos.network",
        recipient_name
    ));
    Ok(())
}

/// List ephemeral messages received
async fn list_messages(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("list_messages", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Get pagination page
    let page = if args.has_argument("page") {
        let page_raw = args.get_value("page")?.to_number()?;
        // Validate page fits in u32 range
        if page_raw > u64::from(u32::MAX) {
            return Err(CommandError::InvalidArgument(
                "Page number exceeds maximum allowed range".to_string(),
            ));
        }
        page_raw as u32
    } else {
        0
    };

    // Get sender's registered name hash
    let name_hash = {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();

        let result = daemon_api
            .get_account_name_hash(&wallet.get_address())
            .await
            .map_err(|e| {
                CommandError::Any(anyhow::anyhow!("Failed to get your registered name: {}", e))
            })?;

        result.name_hash.ok_or_else(|| {
            CommandError::InvalidArgument(
                "You don't have a registered TNS name. Use 'register_name' first.".to_string(),
            )
        })?
    };

    // Query messages (use saturating_mul to prevent overflow on large page values)
    let offset = page.saturating_mul(ELEMENTS_PER_PAGE as u32);
    {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();

        match daemon_api
            .get_messages(&name_hash, offset, ELEMENTS_PER_PAGE as u32)
            .await
        {
            Ok(result) => {
                if result.messages.is_empty() {
                    manager.message("No messages found.");
                } else {
                    manager.message(format!(
                        "Messages (page {}, {} total):",
                        page, result.total_count
                    ));

                    // Import decryption helpers
                    use tos_common::crypto::elgamal::CompressedHandle;
                    use tos_common::serializer::{Reader, Serializer as _};
                    use tos_common::transaction::extra_data::{
                        derive_shared_key_from_handle, Cipher,
                    };

                    for (i, msg) in result.messages.iter().enumerate() {
                        // Try to decrypt message preview
                        let content = {
                            // Read compressed handle from bytes using Serializer
                            let mut reader = Reader::new(&msg.receiver_handle);
                            if let Ok(compressed_handle) = CompressedHandle::read(&mut reader) {
                                if let Ok(handle) = compressed_handle.decompress() {
                                    let shared_key = derive_shared_key_from_handle(
                                        wallet.get_keypair().get_private_key(),
                                        &handle,
                                    );
                                    let cipher = Cipher(msg.encrypted_content.clone());
                                    match cipher.decrypt(&shared_key) {
                                        Ok(plaintext) => {
                                            String::from_utf8_lossy(&plaintext.0).to_string()
                                        }
                                        Err(_) => "[encrypted]".to_string(),
                                    }
                                } else {
                                    "[encrypted]".to_string()
                                }
                            } else {
                                "[encrypted]".to_string()
                            }
                        };

                        // Safe truncation that respects UTF-8 char boundaries
                        let preview = if content.chars().count() > 40 {
                            let truncated: String = content.chars().take(40).collect();
                            format!("{}...", truncated)
                        } else {
                            content
                        };
                        manager.message(format!(
                            "  {}. ID: {} | From: {} | Content: {}",
                            offset as usize + i + 1,
                            msg.message_id,
                            msg.sender_name_hash,
                            preview
                        ));
                    }
                }
            }
            Err(e) => {
                manager.error(format!("Failed to get messages: {}", e));
            }
        }
    }

    Ok(())
}

/// Read a specific ephemeral message
async fn read_message(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.validate_batch_params("read_message", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Get message ID
    let message_id = if args.has_argument("message_id") {
        args.get_value("message_id")?.to_hash()?
    } else {
        return Err(CommandError::MissingArgument("message_id".to_string()));
    };

    manager.message(format!("Reading message {}...", message_id));

    {
        let network_handler = wallet.get_network_handler().lock().await;
        let handler = network_handler.as_ref().ok_or_else(|| {
            CommandError::InvalidArgument("Wallet not connected to daemon".to_string())
        })?;
        let daemon_api = handler.get_api();

        match daemon_api.get_message_by_id(&message_id).await {
            Ok(result) => {
                if let Some(msg) = result.message {
                    // Decrypt message content using wallet's private key
                    use tos_common::crypto::elgamal::CompressedHandle;
                    use tos_common::serializer::{Reader, Serializer as _};
                    use tos_common::transaction::extra_data::{
                        derive_shared_key_from_handle, Cipher,
                    };

                    let decrypted_content = {
                        // Read compressed handle from bytes using Serializer
                        let mut reader = Reader::new(&msg.receiver_handle);
                        let compressed_handle =
                            CompressedHandle::read(&mut reader).map_err(|_| {
                                CommandError::InvalidArgument(
                                    "Invalid receiver handle in message".to_string(),
                                )
                            })?;

                        // Decompress the handle
                        let handle = compressed_handle.decompress().map_err(|_| {
                            CommandError::InvalidArgument(
                                "Failed to decompress receiver handle".to_string(),
                            )
                        })?;

                        // Derive shared key using wallet's private key: k = SHA3-256(sk * handle)
                        let shared_key = derive_shared_key_from_handle(
                            wallet.get_keypair().get_private_key(),
                            &handle,
                        );

                        // Decrypt with ChaCha20
                        let cipher = Cipher(msg.encrypted_content.clone());
                        match cipher.decrypt(&shared_key) {
                            Ok(plaintext) => String::from_utf8_lossy(&plaintext.0).to_string(),
                            Err(_) => {
                                // Decryption failed - message may not be for us or corrupted
                                format!(
                                    "[Decryption failed - raw: {}]",
                                    String::from_utf8_lossy(&msg.encrypted_content)
                                )
                            }
                        }
                    };

                    manager.message("Message details:");
                    manager.message(format!("  ID: {}", msg.message_id));
                    manager.message(format!("  From: {}", msg.sender_name_hash));
                    manager.message(format!("  Nonce: {}", msg.message_nonce));
                    manager.message(format!("  Stored at: block {}", msg.stored_topoheight));
                    manager.message(format!("  Expires at: block {}", msg.expiry_topoheight));
                    manager.message(format!("  Content: {}", decrypted_content));
                } else {
                    manager.message(format!("Message {} not found.", message_id));
                }
            }
            Err(e) => {
                manager.error(format!("Failed to get message: {}", e));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tos_common::prompt::{default_logs_datetime_format, LogLevel, ShareablePrompt};

    fn build_test_prompt() -> Result<ShareablePrompt, CommandError> {
        let temp_dir = std::env::temp_dir().join("tos_wallet_cli_tests");
        fs::create_dir_all(&temp_dir).map_err(|e| CommandError::Any(e.into()))?;
        let dir_path = format!("{}/", temp_dir.display());
        let config = PromptConfig {
            level: LogLevel::Off,
            dir_path: &dir_path,
            filename_log: "test.log",
            disable_file_logging: true,
            disable_file_log_date_based: true,
            disable_colors: true,
            enable_auto_compress_logs: false,
            interactive: false,
            module_logs: Vec::new(),
            file_level: LogLevel::Off,
            show_ascii: false,
            logs_datetime_format: default_logs_datetime_format(),
        };

        Prompt::new(config).map_err(|e| CommandError::Any(e.into()))
    }

    fn noop_handler(_: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
        Ok(())
    }

    #[tokio::test]
    async fn unfreeze_tos_delegate_parses_optional_args() {
        let prompt = build_test_prompt().expect("prompt init");
        let manager = CommandManager::new_with_batch_mode(prompt, true);
        let (required_args, optional_args) = unfreeze_tos_delegate_args();
        manager
            .add_command(Command::with_arguments(
                "unfreeze_tos_delegate",
                "test",
                required_args,
                optional_args,
                CommandHandler::Sync(noop_handler),
            ))
            .expect("register command");

        manager
            .handle_command("unfreeze_tos_delegate amount=1".to_string())
            .await
            .expect("amount only");
        manager
            .handle_command("unfreeze_tos_delegate amount=1 record_index=0".to_string())
            .await
            .expect("record_index only");
        manager
            .handle_command("unfreeze_tos_delegate amount=1 delegatee=addr".to_string())
            .await
            .expect("delegatee only");
        manager
            .handle_command(
                "unfreeze_tos_delegate amount=1 delegatee=addr record_index=0".to_string(),
            )
            .await
            .expect("delegatee and record_index");
    }
}
