use std::{
    fs::File,
    io::Write,
    ops::AddAssign,
    path::Path,
    sync::Arc,
    time::Duration,
    borrow::Cow
};
use anyhow::{Result, Context};
use indexmap::IndexSet;
use log::{error, info, warn};
use clap::Parser;
use sha3::{Digest, Keccak256};
use tos_common::{
    ai_mining::{AIMiningPayload, DifficultyLevel},
    async_handler,
    config::{
        init,
        TOS_ASSET
    },
    crypto::{
        Address,
        Hash,
        Hashable,
        Signature,
        HASH_SIZE
    },
    network::Network,
    prompt::{
        argument::{
            Arg,
            ArgType,
            ArgumentManager
        },
        command::{
            Command,
            CommandError,
            CommandHandler,
            CommandManager
        },
        Color,
        Prompt,
        PromptError
    },
    serializer::Serializer,
    tokio,
    transaction::{
        builder::{FeeBuilder, MultiSigBuilder, TransactionTypeBuilder, TransferBuilder, EnergyBuilder},
        multisig::{MultiSig, SignatureId},
        BurnPayload,

        MultiSigPayload,
        Transaction,
        TxVersion
    },
    utils::{
        format_coin,
        format_tos,
        from_coin
    }
};
use tos_wallet::{
    config::{Config, JsonBatchConfig, LogProgressTableGenerationReportFunction, DIR_PATH},
    entry::EntryData,
    precomputed_tables,
    wallet::{
        RecoverOption,
        Wallet
    }
};

#[cfg(feature = "network_handler")]
use tos_wallet::config::DEFAULT_DAEMON_ADDRESS;

#[cfg(feature = "xswd")]
use {
    tos_wallet::{
        api::{
            AuthConfig,
            PermissionResult,
            AppStateShared
        },
        wallet::XSWDEvent,
    },
    tos_common::{
        rpc::RpcRequest,
        prompt::ShareablePrompt,
        tokio::{
            spawn_task,
            sync::mpsc::UnboundedReceiver
        }
    },
    anyhow::Error,
};

const ELEMENTS_PER_PAGE: usize = 10;

#[tokio::main]
async fn main() -> Result<()> {
    init();

    let mut config: Config = Config::parse();
    if let Some(path) = config.config_file.as_ref() {
        if config.generate_config_template {
            if Path::new(path).exists() {
                eprintln!("Config file already exists at {}", path);
                return Ok(());
            }

            let mut file = File::create(path).context("Error while creating config file")?;
            let json = serde_json::to_string_pretty(&config).context("Error while serializing config file")?;
            file.write_all(json.as_bytes()).context("Error while writing config file")?;
            println!("Config file template generated at {}", path);
            return Ok(());
        }

        let file = File::open(path).context("Error while opening config file")?;
        config = serde_json::from_reader(file).context("Error while reading config file")?;
    } else if config.generate_config_template {
        eprintln!("Provided config file path is required to generate the template with --config-file");
        return Ok(());
    }

    let log_config = &config.log;
    let prompt = Prompt::new(
        log_config.log_level,
        &log_config.logs_path,
        &log_config.filename_log,
        log_config.disable_file_logging,
        log_config.disable_file_log_date_based,
        log_config.disable_log_color,
        log_config.auto_compress_logs,
        !log_config.disable_interactive_mode,
        log_config.logs_modules.clone(),
        log_config.file_log_level.unwrap_or(log_config.log_level),
        !log_config.disable_ascii_art,
        log_config.datetime_format.clone(),
    )?;

    #[cfg(feature = "api_server")]
    {
        // Sanity check
        // check that we don't have both server enabled
        if config.enable_xswd && config.rpc.rpc_bind_address.is_some() {
            error!("Invalid parameters configuration: RPC Server and XSWD cannot be enabled at the same time");
            return Ok(()); // exit
        }

        // check that username/password is not in param if bind address is not set
        if config.rpc.rpc_bind_address.is_none() && (config.rpc.rpc_password.is_some() || config.rpc.rpc_username.is_some()) {
            error!("Invalid parameters configuration for rpc password and username: RPC Server is not enabled");
            return Ok(())
        }

        // check that username/password is set together if bind address is set
        if config.rpc.rpc_bind_address.is_some() && config.rpc.rpc_password.is_some() != config.rpc.rpc_username.is_some() {
            error!("Invalid parameters configuration: usernamd AND password must be provided");
            return Ok(())
        }
    }

    let command_manager = CommandManager::new_with_batch_mode(prompt.clone(), config.is_exec_mode());
    command_manager.store_in_context(config.network)?;

    if let Some(path) = config.wallet_path.as_ref() {
        // read password from option or ask him
        let password = if let Some(password) = config.password.as_ref() {
            password.clone()
        } else if config.is_exec_mode() {
            error!("Exec mode enabled but no password specified. Use --password to specify the password.");
            return Ok(());
        } else {
            prompt.read_input(format!("Enter Password for '{}': ", path), true).await?
        };

        let precomputed_tables = precomputed_tables::read_or_generate_precomputed_tables(config.precomputed_tables.precomputed_tables_path.as_deref(), config.precomputed_tables.precomputed_tables_l1, LogProgressTableGenerationReportFunction, true).await?;
        let p = Path::new(path);
        let wallet = if p.exists() && p.is_dir() && Path::new(&format!("{}/db", path)).exists() {
            if log::log_enabled!(log::Level::Info) {
                info!("Opening wallet {}", path);
            }
            Wallet::open(path, &password, config.network, precomputed_tables, config.n_decryption_threads, config.network_concurrency)?
        } else {
            if log::log_enabled!(log::Level::Info) {
                info!("Creating a new wallet at {}", path);
            }
            Wallet::create(path, &password, config.seed.as_deref().map(RecoverOption::Seed), config.network, precomputed_tables, config.n_decryption_threads, config.network_concurrency).await?
        };

        command_manager.register_default_commands()?;

        apply_config(config.clone(), &wallet, #[cfg(feature = "xswd")] &prompt).await;
        setup_wallet_command_manager(wallet, &command_manager).await?;
        
        // Handle exec mode
        if config.is_exec_mode() {
            if let Some(json_str) = config.json.as_ref() {
                info!("Executing batch command from JSON string");
                execute_json_batch(&command_manager, json_str, &config).await?;
            } else if let Some(json_file) = config.json_file.as_ref() {
                if log::log_enabled!(log::Level::Info) {
                    info!("Executing batch command from JSON file: {}", json_file);
                }
                let json_content = std::fs::read_to_string(json_file)
                    .with_context(|| format!("Failed to read JSON file: {}", json_file))?;
                execute_json_batch(&command_manager, &json_content, &config).await?;
            } else if let Some(cmd) = config.get_exec_command() {
                if log::log_enabled!(log::Level::Info) {
                    info!("Executing command: {}", cmd);
                }
                match command_manager.handle_command(cmd.clone()).await {
                    Ok(_) => {
                        info!("Batch command executed successfully");
                    }
                    Err(e) => {
                        if log::log_enabled!(log::Level::Error) {
                            error!("Error executing batch command: {:#}", e);
                        }
                        return Err(e.into());
                    }
                }
            } else {
                error!("Exec mode enabled but no command specified. Use --exec, --json, or --json-file to specify the command.");
                return Ok(());
            }
        } else {
            // Normal interactive mode
            if let Err(e) = prompt.start(Duration::from_millis(1000), Box::new(async_handler!(prompt_message_builder)), Some(&command_manager)).await {
                if log::log_enabled!(log::Level::Error) {
                    error!("Error while running prompt: {:#}", e);
                }
            }
        }
    } else {
        register_default_commands(&command_manager).await?;

        // Handle exec mode without wallet
        if config.is_exec_mode() {
            if let Some(cmd) = config.get_exec_command() {
                if log::log_enabled!(log::Level::Info) {
                    info!("Executing command: {}", cmd);
                }
                match command_manager.handle_command(cmd.clone()).await {
                    Ok(_) => {
                        info!("Batch command executed successfully");
                    }
                    Err(e) => {
                        if log::log_enabled!(log::Level::Error) {
                            error!("Error executing batch command: {:#}", e);
                        }
                        return Err(e.into());
                    }
                }
            } else {
                error!("Exec mode enabled but no command specified. Use --exec to specify the command.");
                return Ok(());
            }
        } else {
            // Normal interactive mode
            if let Err(e) = prompt.start(Duration::from_millis(1000), Box::new(async_handler!(prompt_message_builder)), Some(&command_manager)).await {
                if log::log_enabled!(log::Level::Error) {
                    error!("Error while running prompt: {:#}", e);
                }
            }
        }
    }

    if let Ok(context) = command_manager.get_context().lock() {
        if let Ok(wallet) = context.get::<Arc<Wallet>>() {
            wallet.close().await;
        }
    }

    Ok(())
}

async fn register_default_commands(manager: &CommandManager) -> Result<(), CommandError> {
    manager.add_command(Command::with_optional_arguments(
        "open",
        "Open a wallet",
        vec![
            Arg::new("name", ArgType::String),
            Arg::new("password", ArgType::String)
        ],
        CommandHandler::Async(async_handler!(open_wallet))
    ))?;

    manager.add_command(Command::with_optional_arguments(
        "create",
        "Create a new wallet",
        vec![
            Arg::new("name", ArgType::String),
            Arg::new("password", ArgType::String),
            Arg::new("confirm_password", ArgType::String)
        ],
        CommandHandler::Async(async_handler!(create_wallet))
    ))?;

    manager.add_command(Command::with_optional_arguments(
        "recover_seed",
        "Recover a wallet using a seed",
        vec![
            Arg::new("name", ArgType::String),
            Arg::new("password", ArgType::String),
            Arg::new("seed", ArgType::String)
        ],
        CommandHandler::Async(async_handler!(recover_seed))
    ))?;

    manager.add_command(Command::with_optional_arguments(
        "recover_private_key",
        "Recover a wallet using a private key",
        vec![
            Arg::new("name", ArgType::String),
            Arg::new("password", ArgType::String),
            Arg::new("private_key", ArgType::String)
        ],
        CommandHandler::Async(async_handler!(recover_private_key))
    ))?;

    manager.register_default_commands()?;
    // Display available commands
    manager.display_commands()?;

    Ok(())
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
            },
            XSWDEvent::RequestApplication(app_state, callback) => {
                let prompt = prompt.clone();
                let res = xswd_handle_request_application(&prompt, app_state).await;
                if callback.send(res).is_err() {
                    error!("Error while sending application response back to XSWD");
                }
            },
            XSWDEvent::RequestPermission(app_state, request, callback) => {
                let res = xswd_handle_request_permission(&prompt, app_state, request).await;
                if callback.send(res).is_err() {
                    error!("Error while sending permission response back to XSWD");
                }
            },
            XSWDEvent::AppDisconnect(_) => {}
        };
    }
}

#[cfg(feature = "xswd")]
async fn xswd_handle_request_application(prompt: &ShareablePrompt, app_state: AppStateShared) -> Result<PermissionResult, Error> {
    let mut message = format!("XSWD: Application {} ({}) request access to your wallet", app_state.get_name(), app_state.get_id());
    let permissions = app_state.get_permissions().lock().await;
    if !permissions.is_empty() {
        message += &format!("\r\nPermissions ({}):", permissions.len());
        for perm in permissions.keys() {
            message += &format!("\r\n- {}", perm);
        }
    }

    message += "\r\n(Y/N): ";
    let accepted = prompt.read_valid_str_value(prompt.colorize_string(Color::Blue, &message), vec!["y", "n"]).await? == "y";
    if accepted {
        Ok(PermissionResult::Accept)
    } else {
        Ok(PermissionResult::Reject)
    }
}

#[cfg(feature = "xswd")]
async fn xswd_handle_request_permission(prompt: &ShareablePrompt, app_state: AppStateShared, request: RpcRequest) -> Result<PermissionResult, Error> {
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

    let answer = prompt.read_valid_str_value(prompt.colorize_string(Color::Blue, &message), vec!["a", "d", "aa", "ad"]).await?;
    Ok(match answer.as_str() {
        "a" => PermissionResult::Accept,
        "d" => PermissionResult::Reject,
        "aa" => PermissionResult::AlwaysAccept,
        "ad" => PermissionResult::AlwaysReject,
        _ => unreachable!()
    })
}

// Apply the config passed in params
async fn apply_config(config: Config, wallet: &Arc<Wallet>, #[cfg(feature = "xswd")] prompt: &ShareablePrompt) {
    #[cfg(feature = "network_handler")]
    if !config.network_handler.offline_mode {
        if log::log_enabled!(log::Level::Info) {
            info!("Trying to connect to daemon at '{}'", config.network_handler.daemon_address);
        }
        if let Err(e) = wallet.set_online_mode(&config.network_handler.daemon_address, true).await {
            if log::log_enabled!(log::Level::Error) {
                error!("Couldn't connect to daemon: {:#}", e);
            }
            info!("You can activate online mode using 'online_mode [daemon_address]'");
        } else {
            info!("Online mode enabled");
        }
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
            let auth_config = if let (Some(username), Some(password)) = (config.rpc.rpc_username, config.rpc.rpc_password) {
                Some(AuthConfig {
                    username,
                    password
                })
            } else {
                None
            };

            if log::log_enabled!(log::Level::Info) {
                info!("Enabling RPC Server on {} {}", address, if auth_config.is_some() { "with authentication" } else { "without authentication" });
            }
            if let Err(e) = wallet.enable_rpc_server(address, auth_config, config.rpc.rpc_threads).await {
                if log::log_enabled!(log::Level::Error) {
                    error!("Error while enabling RPC Server: {:#}", e);
                }
            }
        } else if config.enable_xswd {
            match wallet.enable_xswd().await {
                Ok(receiver) => {
                    if let Some(receiver) = receiver {
                        // Only clone when its necessary
                        let prompt = prompt.clone();
                        spawn_task("xswd-handler", xswd_handler(receiver, prompt));
                    }
                },
                Err(e) => error!("Error while enabling XSWD Server: {}", e)
            };
        }
    }
}

// Function to build the CommandManager when a wallet is open
async fn setup_wallet_command_manager(wallet: Arc<Wallet>, command_manager: &CommandManager) -> Result<(), CommandError> {
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
            Arg::new("old_password", ArgType::String),
            Arg::new("new_password", ArgType::String)
        ],
        CommandHandler::Async(async_handler!(change_password))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "transfer",
        "Send asset to a specified address",
        vec![
            Arg::new("asset", ArgType::Hash),
            Arg::new("address", ArgType::String),
            Arg::new("amount", ArgType::String),
            Arg::new("fee_type", ArgType::String),
            Arg::new("confirm", ArgType::String)
        ],
        CommandHandler::Async(async_handler!(transfer))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "transfer_all",
        "Send all your asset balance to a specified address",
        vec![
            Arg::new("asset", ArgType::Hash),
            Arg::new("address", ArgType::String),
            Arg::new("fee_type", ArgType::String),
            Arg::new("confirm", ArgType::Bool)
        ],
        CommandHandler::Async(async_handler!(transfer_all))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "burn",
        "Burn amount of asset",
        vec![
            Arg::new("asset", ArgType::Hash),
            Arg::new("amount", ArgType::String),
            Arg::new("confirm", ArgType::Bool)
        ],    
        CommandHandler::Async(async_handler!(burn))
    ))?;
    command_manager.add_command(Command::new(
        "display_address",
        "Show your wallet address",
        CommandHandler::Async(async_handler!(display_address))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "balance",
        "Show the balance of requested asset; Asset must be tracked",
        vec![Arg::new("asset", ArgType::Hash)],
        CommandHandler::Async(async_handler!(balance))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "history",
        "Show all your transactions",
        vec![Arg::new("page", ArgType::Number)],
        CommandHandler::Async(async_handler!(history))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "transaction",
        "Show a specific transaction",
        vec![Arg::new("hash", ArgType::Hash)],
        CommandHandler::Async(async_handler!(transaction))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "seed",
        "Show seed of selected language",
        vec![
            Arg::new("language", ArgType::Number),
            Arg::new("password", ArgType::String)
        ],
        CommandHandler::Async(async_handler!(seed))
    ))?;
    command_manager.add_command(Command::new(
        "nonce",
        "Show current nonce",
        CommandHandler::Async(async_handler!(nonce))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "set_nonce",
        "Set new nonce",
        vec![Arg::new("nonce", ArgType::Number)],
        CommandHandler::Async(async_handler!(set_nonce))
    ))?;
    command_manager.add_command(Command::new(
        "logout",
        "Logout from existing wallet",
        CommandHandler::Async(async_handler!(logout)))
    )?;
    command_manager.add_command(Command::new(
        "clear_tx_cache",
        "Clear the current TX cache",
        CommandHandler::Async(async_handler!(clear_tx_cache))
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "export_transactions",
        "Export all your transactions in a CSV file",
        vec![Arg::new("filename", ArgType::String)],
        CommandHandler::Async(async_handler!(export_transactions_csv))
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "freeze_tos",
        "Freeze TOS to get energy with duration-based rewards (3/7/14 days)",
        vec![
            Arg::new("amount", ArgType::String),
            Arg::new("duration", ArgType::Number),
            Arg::new("confirm", ArgType::String)
        ],
        CommandHandler::Async(async_handler!(freeze_tos))
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "unfreeze_tos",
        "Unfreeze TOS (release frozen TOS after lock period)",
        vec![
            Arg::new("amount", ArgType::String),
            Arg::new("confirm", ArgType::String)
        ],
        CommandHandler::Async(async_handler!(unfreeze_tos))
    ))?;
    command_manager.add_command(Command::new(
        "energy_info",
        "Show energy information and freeze records",
        CommandHandler::Async(async_handler!(energy_info))
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "set_asset_name",
        "Set the name of an asset",
        vec![
            Arg::new("asset", ArgType::Hash),
            Arg::new("name", ArgType::String)
        ],
        CommandHandler::Async(async_handler!(set_asset_name))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "list_assets",
        "List all detected assets",
        vec![Arg::new("page", ArgType::Number)],
        CommandHandler::Async(async_handler!(list_assets))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "list_balances",
        "List all balances tracked",
        vec![Arg::new("page", ArgType::Number)],
        CommandHandler::Async(async_handler!(list_balances))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "list_tracked_assets",
        "List all assets marked as tracked",
        vec![Arg::new("page", ArgType::Number)],
        CommandHandler::Async(async_handler!(list_tracked_assets))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "track_asset",
        "Mark an asset hash as tracked",
        vec![Arg::new("asset", ArgType::Hash)],
        CommandHandler::Async(async_handler!(track_asset))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "untrack_asset",
        "Remove an asset hash from being tracked",
        vec![Arg::new("asset", ArgType::Hash)],
        CommandHandler::Async(async_handler!(untrack_asset))
    ))?;

    #[cfg(feature = "network_handler")]
    {
        command_manager.add_command(Command::with_optional_arguments(
            "online_mode",
            "Set your wallet in online mode",
            vec![Arg::new("daemon_address", ArgType::String)],
            CommandHandler::Async(async_handler!(online_mode))
        ))?;
        command_manager.add_command(Command::new(
            "offline_mode",
            "Set your wallet in offline mode",
            CommandHandler::Async(async_handler!(offline_mode))
        ))?;
        command_manager.add_command(Command::with_optional_arguments(
            "rescan",
            "Rescan balance and transactions",
            vec![Arg::new("topoheight", ArgType::Number)],
            CommandHandler::Async(async_handler!(rescan))
        ))?;
    }

    #[cfg(feature = "api_server")]
    {
        // Unauthenticated RPC Server can only be created by launch arguments option
        command_manager.add_command(Command::with_required_arguments(
            "start_rpc_server",
            "Start the RPC Server",
            vec![
                Arg::new("bind_address", ArgType::String),
                Arg::new("username", ArgType::String),
                Arg::new("password", ArgType::String)
            ], CommandHandler::Async(async_handler!(start_rpc_server))))?;

        command_manager.add_command(Command::new(
            "start_xswd",
            "Start the XSWD Server",
            CommandHandler::Async(async_handler!(start_xswd)))
        )?;

        // Stop API Server (RPC or XSWD)
        command_manager.add_command(Command::new(
            "stop_api_server",
            "Stop the API (XSWD/RPC) Server",
            CommandHandler::Async(async_handler!(stop_api_server)))
        )?;
    }

    #[cfg(feature = "xswd")]
    {
        command_manager.add_command(Command::with_optional_arguments(
            "add_xswd_relayer",
            "Add a XSWD relayer to the wallet",
            vec![Arg::new("app_data", ArgType::String)],
            CommandHandler::Async(async_handler!(add_xswd_relayer))
        ))?;
    }

    // Also add multisig commands
    command_manager.add_command(Command::with_optional_arguments(
        "multisig_setup",
        "Setup a multisig",
        vec![
            Arg::new("participants", ArgType::Number),
            Arg::new("threshold", ArgType::Number),
            Arg::new("confirm", ArgType::Bool)
        ],
        CommandHandler::Async(async_handler!(multisig_setup))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "multisig_sign",
        "Sign a multisig transaction",
        vec![
            Arg::new("tx_hash", ArgType::Hash)
        ],
        CommandHandler::Async(async_handler!(multisig_sign))
    ))?;
    command_manager.add_command(Command::new(
        "multisig_show",
        "Show the current state of multisig",
        CommandHandler::Async(async_handler!(multisig_show))
    ))?;

    command_manager.add_command(Command::new(
        "tx_version",
        "See the current transaction version",
        CommandHandler::Async(async_handler!(tx_version))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "set_tx_version",
        "Set the transaction version",
        vec![Arg::new("version", ArgType::Number)],
        CommandHandler::Async(async_handler!(set_tx_version))
    ))?;
    command_manager.add_command(Command::new(
        "status",
        "See the status of the wallet",
        CommandHandler::Async(async_handler!(status))
    ))?;

    // AI Mining commands
    command_manager.add_command(Command::with_optional_arguments(
        "ai_mining_history",
        "Show AI mining transaction history",
        vec![
            Arg::new("page", ArgType::Number),
            Arg::new("limit", ArgType::Number),
            Arg::new("type", ArgType::String)
        ],
        CommandHandler::Async(async_handler!(ai_mining_history))
    ))?;
    command_manager.add_command(Command::new(
        "ai_mining_stats",
        "Show your AI mining statistics",
        CommandHandler::Async(async_handler!(ai_mining_stats))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "ai_mining_tasks",
        "Show AI mining tasks you've published or participated in",
        vec![
            Arg::new("page", ArgType::Number),
            Arg::new("status", ArgType::String)
        ],
        CommandHandler::Async(async_handler!(ai_mining_tasks))
    ))?;
    command_manager.add_command(Command::with_optional_arguments(
        "ai_mining_rewards",
        "Show AI mining rewards earned",
        vec![Arg::new("page", ArgType::Number)],
        CommandHandler::Async(async_handler!(ai_mining_rewards))
    ))?;

    // AI Mining business commands
    command_manager.add_command(Command::with_required_arguments(
        "publish_task",
        "Publish a new AI mining task",
        vec![
            Arg::new("description", ArgType::String),
            Arg::new("reward", ArgType::Number),
            Arg::new("difficulty", ArgType::String),
            Arg::new("deadline", ArgType::Number)
        ],
        CommandHandler::Async(async_handler!(publish_task))
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "submit_answer",
        "Submit answer to an AI mining task",
        vec![
            Arg::new("task_id", ArgType::String),
            Arg::new("answer_content", ArgType::String),
            Arg::new("answer_hash", ArgType::String),
            Arg::new("stake", ArgType::Number)
        ],
        CommandHandler::Async(async_handler!(submit_answer))
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "validate_answer",
        "Validate a submitted answer",
        vec![
            Arg::new("task_id", ArgType::String),
            Arg::new("answer_id", ArgType::String),
            Arg::new("score", ArgType::Number)
        ],
        CommandHandler::Async(async_handler!(validate_answer))
    ))?;
    command_manager.add_command(Command::with_required_arguments(
        "register_miner",
        "Register as an AI miner",
        vec![Arg::new("fee", ArgType::Number)],
        CommandHandler::Async(async_handler!(register_miner))
    ))?;

    let mut context = command_manager.get_context().lock()?;
    context.store(wallet);

    command_manager.display_commands()
}

// Function passed as param to prompt to build the prompt message shown
async fn prompt_message_builder(prompt: &Prompt, command_manager: Option<&CommandManager>) -> Result<String, PromptError> {
    if let Some(manager) = command_manager {
        let context = manager.get_context().lock()?;
        if let Ok(wallet) = context.get::<Arc<Wallet>>() {
            let network = wallet.get_network();

            let addr_str = {
                let addr = &wallet.get_address().to_string()[..8];
                prompt.colorize_string(Color::Yellow, addr)
            };
    
            let storage = wallet.get_storage().read().await;
            let topoheight_str = format!(
                "{}: {}",
                prompt.colorize_string(Color::Yellow, "TopoHeight"),
                prompt.colorize_string(Color::Green, &format!("{}", storage.get_synced_topoheight().unwrap_or(0)))
            );
            let balance = format!(
                "{}: {}",
                prompt.colorize_string(Color::Yellow, "Balance"),
                prompt.colorize_string(Color::Green, &format_tos(storage.get_plaintext_balance_for(&TOS_ASSET).await.unwrap_or(0))),
            );
            let status = if wallet.is_online().await {
                prompt.colorize_string(Color::Green, "Online")
            } else {
                prompt.colorize_string(Color::Red, "Offline")
            };
            let network_str = if !network.is_mainnet() {
                format!(
                    "{} ",
                    prompt.colorize_string(Color::Red, &network.to_string())
                )
            } else { "".into() };
    
            return Ok(
                format!(
                    "{} | {} | {} | {} | {} {}{} ",
                    prompt.colorize_string(Color::Blue, "Tos Wallet"),
                    addr_str,
                    topoheight_str,
                    balance,
                    status,
                    network_str,
                    prompt.colorize_string(Color::BrightBlack, ">>")
                )
            )
        }
    }

    Ok(
        format!(
            "{} {} ",
            prompt.colorize_string(Color::Blue, "Tos Wallet"),
            prompt.colorize_string(Color::BrightBlack, ">>")
        )
    )
}

// Open a wallet based on the wallet name and its password
async fn open_wallet(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("open", &args)?;

    let prompt = manager.get_prompt();
    let config: Config = Config::parse();

    // Priority: command line args -> config file -> interactive prompt (only if not batch mode)
    let dir = if args.has_argument("name") {
        let name = args.get_value("name")?.to_string_value()?;
        format!("{}{}", DIR_PATH, name)
    } else if let Some(path) = config.wallet_path.as_ref() {
        path.clone()
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("name".to_string()));
    } else {
        let name = prompt.read_input("Wallet name: ", false)
            .await.context("Error while reading wallet name")?;

        if name.is_empty() {
            manager.error("Wallet name cannot be empty");
            return Ok(())
        }
        format!("{}{}", DIR_PATH, name)
    };

    if !Path::new(&dir).is_dir() {
        manager.message("No wallet found with this name");
        return Ok(())
    }

    let password = if args.has_argument("password") {
        args.get_value("password")?.to_string_value()?
    } else if let Some(pwd) = config.password.as_ref() {
        pwd.clone()
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("password".to_string()));
    } else {
        prompt.read_input("Password: ", true)
            .await.context("Error while reading wallet password")?
    };

    let wallet = {
        let context = manager.get_context().lock()?;
        let network = context.get::<Network>()?;
        let precomputed_tables = precomputed_tables::read_or_generate_precomputed_tables(config.precomputed_tables.precomputed_tables_path.as_deref(), config.precomputed_tables.precomputed_tables_l1, LogProgressTableGenerationReportFunction, true).await?;
        Wallet::open(&dir, &password, *network, precomputed_tables, config.n_decryption_threads, config.network_concurrency)?
    };

    manager.message("Wallet sucessfully opened");
    apply_config(config, &wallet, #[cfg(feature = "xswd")] &prompt).await;

    setup_wallet_command_manager(wallet, manager).await?;

    Ok(())
}

// Create a wallet by requesting name, password
async fn create_wallet(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("create", &args)?;

    let prompt = manager.get_prompt();
    let config: Config = Config::parse();

    // Priority: command line args -> config file -> interactive prompt (only if not batch mode)
    let dir = if args.has_argument("name") {
        let name = args.get_value("name")?.to_string_value()?;
        format!("{}{}", DIR_PATH, name)
    } else if let Some(path) = config.wallet_path.as_ref() {
        path.clone()
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("name".to_string()));
    } else {
        let name = prompt.read_input("Wallet name: ", false)
            .await.context("Error while reading wallet name")?;

        if name.is_empty() {
            manager.error("Wallet name cannot be empty");
            return Ok(())
        }
        format!("{}{}", DIR_PATH, name)
    };

    if Path::new(&dir).is_dir() {
        manager.message("wallet already exists with this name");
        return Ok(())
    }

    // Handle password input with batch mode support
    let password = if args.has_argument("password") {
        args.get_value("password")?.to_string_value()?
    } else if let Some(pwd) = config.password.as_ref() {
        pwd.clone()
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("password".to_string()));
    } else {
        let password = prompt.read_input("Password: ", true)
            .await.context("Error while reading password")?;
        let confirm_password = prompt.read_input("Confirm Password: ", true)
            .await.context("Error while reading password")?;

        if password != confirm_password {
            manager.message("Confirm password doesn't match password");
            return Ok(())
        }
        password
    };

    let wallet = {
        let context = manager.get_context().lock()?;
        let network = context.get::<Network>()?;
        let precomputed_tables = precomputed_tables::read_or_generate_precomputed_tables(config.precomputed_tables.precomputed_tables_path.as_deref(), precomputed_tables::L1_FULL, LogProgressTableGenerationReportFunction, true).await?;
        Wallet::create(&dir, &password, None, *network, precomputed_tables, config.n_decryption_threads, config.network_concurrency).await?
    };
 
    manager.message("Wallet sucessfully created");
    apply_config(config, &wallet, #[cfg(feature = "xswd")] prompt).await;

    // Display the seed in prompt
    {
        let seed = wallet.get_seed(0)?; // TODO language index
        if manager.is_batch_mode() {
            manager.message(format!("Seed: {}", seed));
            manager.message("IMPORTANT: Please save this seed phrase in a secure location.");
        } else {
            prompt.read_input(format!("Seed: {}\r\nPress ENTER to continue", seed), false)
                .await.context("Error while displaying seed")?;
        }
    }

    setup_wallet_command_manager(wallet, manager).await?;

    Ok(())
}

// Recover a wallet by requesting its seed or private key, name and password
async fn recover_wallet(manager: &CommandManager, mut args: ArgumentManager, seed: bool) -> Result<(), CommandError> {
    let prompt = manager.get_prompt();
    let config: Config = Config::parse();
    // Priority: command line args -> config file -> interactive prompt (only if not batch mode)
    let dir = if args.has_argument("name") {
        let name = args.get_value("name")?.to_string_value()?;
        format!("{}{}", DIR_PATH, name)
    } else if let Some(path) = config.wallet_path.as_ref() {
        path.clone()
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("name".to_string()));
    } else {
        let name = prompt.read_input("Wallet name: ", false)
            .await.context("Error while reading wallet name")?;

        if name.is_empty() {
            manager.error("Wallet name cannot be empty");
            return Ok(())
        }
        format!("{}{}", DIR_PATH, name)
    };

    if Path::new(&dir).is_dir() {
        manager.message("Wallet already exists with this name");
        return Ok(())
    }

    let content = if seed {
        let seed = if args.has_argument("seed") {
            args.get_value("seed")?.to_string_value()?
        } else if let Some(s) = config.seed.as_ref() {
            s.clone()
        } else if manager.is_batch_mode() {
            return Err(CommandError::MissingArgument("seed".to_string()));
        } else {
            prompt.read_input("Seed: ", false)
                .await.context("Error while reading seed")?
        };

        let words_count = seed.split_whitespace().count();
        if words_count != 25 && words_count != 24 {
            manager.error("Seed must be 24 or 25 (checksum) words long");
            return Ok(())
        }
        seed
    } else {
        let private_key = if args.has_argument("private_key") {
            args.get_value("private_key")?.to_string_value()?
        } else if manager.is_batch_mode() {
            return Err(CommandError::MissingArgument("private_key".to_string()));
        } else {
            prompt.read_input("Private Key: ", false)
                .await.context("Error while reading private key")?
        };

        if private_key.len() != 64 {
            manager.error("Private key must be 64 characters long");
            return Ok(())
        }
        private_key
    };

    // Handle password input with batch mode support
    let password = if args.has_argument("password") {
        args.get_value("password")?.to_string_value()?
    } else if let Some(pwd) = config.password.as_ref() {
        pwd.clone()
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("password".to_string()));
    } else {
        let password = prompt.read_input("Password: ", true)
            .await.context("Error while reading password")?;
        let confirm_password = prompt.read_input("Confirm Password: ", true)
            .await.context("Error while reading password")?;

        if password != confirm_password {
            manager.message("Confirm password doesn't match password");
            return Ok(())
        }
        password
    };

    let wallet = {
        let context = manager.get_context().lock()?;
        let network = context.get::<Network>()?;
        let precomputed_tables = precomputed_tables::read_or_generate_precomputed_tables(config.precomputed_tables.precomputed_tables_path.as_deref(), config.precomputed_tables.precomputed_tables_l1, LogProgressTableGenerationReportFunction, true).await?;

        let recover = if seed {
            RecoverOption::Seed(&content)
        } else {
            RecoverOption::PrivateKey(&content)
        };
        Wallet::create(&dir, &password, Some(recover), *network, precomputed_tables, config.n_decryption_threads, config.network_concurrency).await?
    };

    manager.message("Wallet sucessfully recovered");
    apply_config(config, &wallet, #[cfg(feature = "xswd")] prompt).await;

    setup_wallet_command_manager(wallet, manager).await?;

    Ok(())
}

async fn recover_seed(manager: &CommandManager, args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("recover_seed", &args)?;
    recover_wallet(manager, args, true).await
}

async fn recover_private_key(manager: &CommandManager, args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("recover_private_key", &args)?;
    recover_wallet(manager, args, false).await
}

// Set the asset name
async fn set_asset_name(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("set_asset_name", &args)?;

    let prompt = manager.get_prompt();
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let asset = args.get_value("asset")?.to_hash()?;
    let name = if args.has_argument("name") {
        args.get_value("name")?.to_string_value()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("name".to_string()));
    } else {
        prompt.read_input("Asset name: ", false)
            .await.context("Error while reading asset name")?
    };

    let mut storage = wallet.get_storage().write().await;
    storage.set_asset_name(&asset, name).await?;
    manager.message("Asset name has been set");
    Ok(())
}

async fn list_assets(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let page = if args.has_argument("page") {
        args.get_value("page")?.to_number()? as usize
    } else {
        0
    };

    let storage = wallet.get_storage().read().await;
    let count = storage.get_assets_count()?;

    if count == 0 {
        manager.message("No assets found");
        return Ok(())
    }

    let mut max_pages = count / ELEMENTS_PER_PAGE;
    if count % ELEMENTS_PER_PAGE != 0 {
        max_pages += 1;
    }

    if page > max_pages {
        return Err(CommandError::InvalidArgument(format!("Page must be less than maximum pages ({})", max_pages - 1)));
    }

    manager.message(format!("Assets (page {}/{}):", page, max_pages));
    for res in storage.get_assets_with_data().await?.skip(page * ELEMENTS_PER_PAGE).take(ELEMENTS_PER_PAGE) {
        let (asset, data) = res?;
        manager.message(format!("{} ({} decimals): {}", asset, data.get_decimals(), data.get_name()));
    }

    Ok(())
}

async fn list_balances(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let page = if args.has_argument("page") {
        args.get_value("page")?.to_number()? as usize
    } else {
        0
    };

    let storage = wallet.get_storage().read().await;
    let count = storage.get_tracked_assets_count()?;

    if count == 0 {
        manager.message("No balances found");
        return Ok(())
    }

    let mut max_pages = count / ELEMENTS_PER_PAGE;
    if count % ELEMENTS_PER_PAGE != 0 {
        max_pages += 1;
    }

    if page > max_pages {
        return Err(CommandError::InvalidArgument(format!("Page must be less than maximum pages ({})", max_pages - 1)));
    }

    manager.message(format!("Balances (page {}/{}):", page, max_pages));
    for res in storage.get_tracked_assets()?.skip(page * ELEMENTS_PER_PAGE).take(ELEMENTS_PER_PAGE) {
        let asset = res?;
        if let Some(data) = storage.get_optional_asset(&asset).await? {
            let balance = storage.get_plaintext_balance_for(&asset).await?;
            manager.message(format!("Balance for asset {} ({}): {}", data.get_name(), asset, format_coin(balance, data.get_decimals())));
        } else {
            manager.message(format!("No asset data for {}", asset));
        }

    }

    Ok(())
}

async fn list_tracked_assets(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let page = if args.has_argument("page") {
        args.get_value("page")?.to_number()? as usize
    } else {
        0
    };

    let storage = wallet.get_storage().read().await;

    let count = storage.get_tracked_assets_count()?;
    if count == 0 {
        manager.message("No tracked assets found");
        return Ok(())
    }

    let mut max_pages = count / ELEMENTS_PER_PAGE;
    if count % ELEMENTS_PER_PAGE != 0 {
        max_pages += 1;
    }

    if page > max_pages {
        return Err(CommandError::InvalidArgument(format!("Page must be less than maximum pages ({})", max_pages - 1)));
    }

    manager.message(format!("Assets (page {}/{}):", page, max_pages));
    for res in storage.get_tracked_assets()?.skip(page * ELEMENTS_PER_PAGE).take(ELEMENTS_PER_PAGE) {
        let asset = res?;
        if let Some(data) = storage.get_optional_asset(&asset).await? {
            manager.message(format!("{} ({} decimals): {}", asset, data.get_decimals(), data.get_name()));
        } else {
            manager.message(format!("No asset data for {}", asset));
        }
    }

    Ok(())
}

async fn track_asset(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("track_asset", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let prompt = manager.get_prompt();

    let asset = if args.has_argument("asset") {
        args.get_value("asset")?.to_hash()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("asset".to_string()));
    } else {
        prompt.read_hash(prompt.colorize_string(Color::BrightGreen, "Asset ID: ")).await?
    };

    if wallet.track_asset(asset).await.context("Error while tracking asset")? {
        manager.message("Asset ID is already tracked!");
    } else {
        manager.message("Asset ID is now tracked");
    }

    Ok(())
}

async fn untrack_asset(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("untrack_asset", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let prompt = manager.get_prompt();

    let asset = if args.has_argument("asset") {
        args.get_value("asset")?.to_hash()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("asset".to_string()));
    } else {
        prompt.read_hash(prompt.colorize_string(Color::BrightGreen, "Asset ID: ")).await?
    };

    if asset == TOS_ASSET {
        manager.message("TOS asset cannot be untracked");
    } else if wallet.untrack_asset(asset).await.context("Error while untracking asset")? {
        manager.message("Asset ID is not marked as tracked!");
    } else {
        manager.message("Asset ID is not tracked anymore");
    }

    Ok(())
}

// Change wallet password
async fn change_password(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("change_password", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let prompt = manager.get_prompt();

    let old_password = if args.has_argument("old_password") {
        args.get_value("old_password")?.to_string_value()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("old_password".to_string()));
    } else {
        prompt.read_input(prompt.colorize_string(Color::BrightRed, "Current Password: "), true)
            .await
            .context("Error while asking old password")?
    };

    let new_password = if args.has_argument("new_password") {
        args.get_value("new_password")?.to_string_value()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("new_password".to_string()));
    } else {
        prompt.read_input(prompt.colorize_string(Color::BrightRed, "New Password: "), true)
            .await
            .context("Error while asking new password")?
    };

    manager.message("Changing password...");
    wallet.set_password(&old_password, &new_password).await?;
    manager.message("Your password has been changed!");
    Ok(())
}

async fn create_transaction_with_multisig(manager: &CommandManager, prompt: &Prompt, wallet: &Wallet, tx_type: TransactionTypeBuilder, payload: MultiSigPayload) -> Result<Transaction, CommandError> {
    manager.message(format!("Multisig detected, you need to sign the transaction with {} keys.", payload.threshold));

    let mut storage = wallet.get_storage().write().await;
    let fee = FeeBuilder::default();
    let mut state = wallet.create_transaction_state_with_storage(&storage, &tx_type, &fee, None).await
        .context("Error while creating transaction state")?;

    let mut unsigned = wallet.create_unsigned_transaction(&mut state, Some(payload.threshold), tx_type, fee, storage.get_tx_version().await?)
        .context("Error while building unsigned transaction")?;

    let mut multisig = MultiSig::new();
    manager.message(format!("Transaction hash to sign: {}", unsigned.get_hash_for_multisig()));

    if payload.threshold == 1 {
        let signature = prompt.read_input("Enter signature hexadecimal: ", false).await
            .context("Error while reading signature")?;
        let signature = Signature::from_hex(&signature).context("Invalid signature")?;

        let id = if payload.participants.len() == 1 {
            0
        } else {
            prompt.read("Enter signer ID: ").await
            .context("Error while reading signer id")?
        };

        if !multisig.add_signature(SignatureId {
            id,
            signature
        }) {
            return Err(CommandError::InvalidArgument("Invalid signature".to_string()));
        }        
    } else {
        manager.message("Participants available:");
        for (i, participant) in payload.participants.iter().enumerate() {
            manager.message(format!("Participant #{}: {}", i, participant.as_address(wallet.get_network().is_mainnet())));
        }
        
        manager.message("Please enter the signatures and signer IDs");
        for i in 0..payload.threshold {
            let signature = prompt.read_input(format!("Enter signature #{} hexadecimal: ", i), false).await
                .context("Error while reading signature")?;
            let signature = Signature::from_hex(&signature).context("Invalid signature")?;
    
            let id = prompt.read("Enter signer ID for signature: ").await
                .context("Error while reading signer id")?;
    
            if !multisig.add_signature(SignatureId {
                id,
                signature
            }) {
                return Err(CommandError::InvalidArgument("Invalid signature".to_string()));
            }
        }
    }

    unsigned.set_multisig(multisig);

    let tx = unsigned.finalize(wallet.get_keypair());
    state.set_tx_hash_built(tx.hash());

    state.apply_changes(&mut storage).await.context("Error while applying changes")?;

    Ok(tx)
}

// Create a new transfer to a specified address
async fn transfer(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("transfer", &args)?;

    let prompt = manager.get_prompt();
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // read address
    let str_address = if args.has_argument("address") {
        args.get_value("address")?.to_string_value()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("address".to_string()));
    } else {
        prompt.read_input(
            prompt.colorize_string(Color::Green, "Address: "),
            false
        ).await.context("Error while reading address")?
    };
    let address = Address::from_string(&str_address).context("Invalid address")?;

    let asset = if args.has_argument("asset") {
        args.get_value("asset")?.to_hash()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("asset".to_string()));
    } else {
        let asset_name = prompt.read_input(
            prompt.colorize_string(Color::Green, "Asset (default TOS): "),
            false
        ).await?;
        if asset_name.is_empty() {
            TOS_ASSET
        } else if asset_name.len() == HASH_SIZE * 2 {
            Hash::from_hex(&asset_name).context("Error while reading hash from hex")?
        } else {
            let storage = wallet.get_storage().read().await;
            storage.get_asset_by_name(&asset_name).await?
                .context("No asset registered with given name")?
        }
    };

    let (max_balance, asset_data, multisig) = {
        let storage = wallet.get_storage().read().await;
        let balance = storage.get_plaintext_balance_for(&asset).await.unwrap_or(0);
        let asset = storage.get_asset(&asset).await?;
        let multisig = storage.get_multisig_state().await.context("Error while reading multisig state")?;
        (balance, asset, multisig.cloned())
    };

    // read amount
    let amount = if args.has_argument("amount") {
        args.get_value("amount")?.to_string_value()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("amount".to_string()));
    } else {
        prompt.read(
            prompt.colorize_string(Color::Green, &format!("Amount (max: {}): ", format_coin(max_balance, asset_data.get_decimals())))
        ).await.context("Error while reading amount")?
    };

    let amount = from_coin(amount, asset_data.get_decimals()).context("Invalid amount")?;
    
    // Read fee_type parameter
    let fee_type = if args.has_argument("fee_type") {
        let fee_type_str = args.get_value("fee_type")?.to_string_value()?;
        match fee_type_str.to_lowercase().as_str() {
            "tos" => Some(tos_common::transaction::FeeType::TOS),
            "energy" => Some(tos_common::transaction::FeeType::Energy),
            _ => {
                manager.error("Invalid fee_type. Use 'tos' or 'energy'");
                return Ok(());
            }
        }
    } else {
        None
    };

    // Validate fee_type for energy
    if fee_type.as_ref() == Some(&tos_common::transaction::FeeType::Energy) {
        if asset != TOS_ASSET {
            manager.error("Energy fees can only be used for TOS transfers");
            return Ok(());
        }
    }

    manager.message(format!("Sending {} of {} ({}) to {}", format_coin(amount, asset_data.get_decimals()), asset_data.get_name(), asset, address.to_string()));

    // Parse confirmation
    let confirmed = if args.has_argument("confirm") {
        let confirm_str = args.get_value("confirm")?.to_string_value()?;
        match confirm_str.to_lowercase().as_str() {
            "yes" | "y" | "true" => true,
            "no" | "n" | "false" => false,
            _ => {
                let message = format!(
                    "Send {} of {} ({}) to {}?\n(Y/N): ",
                    format_coin(amount, asset_data.get_decimals()),
                    asset_data.get_name(),
                    asset,
                    address.to_string()
                );
                prompt.read_valid_str_value(
                    prompt.colorize_string(Color::Yellow, &message),
                    vec!["y", "n"]
                ).await.context("Error while reading confirmation")? == "y"
            }
        }
    } else if manager.is_batch_mode() {
        true  // Auto-confirm in batch mode when no explicit confirmation parameter
    } else {
        prompt.ask_confirmation().await.context("Error while confirming action")?
    };

    if !confirmed {
        manager.message("Transaction has been aborted");
        return Ok(())
    }

    manager.message("Building transaction...");
    let transfer = TransferBuilder {
        destination: address,
        amount,
        asset,
        extra_data: None,
    };
    let tx_type = TransactionTypeBuilder::Transfers(vec![transfer]);
    
    // Create transaction with appropriate fee type
    let tx = if let Some(multisig) = multisig {
        create_transaction_with_multisig(manager, prompt, wallet, tx_type, multisig.payload).await?
    } else {
        // Create transaction state and builder
        let storage = wallet.get_storage().read().await;
        let mut state = wallet.create_transaction_state_with_storage(&storage, &tx_type, &FeeBuilder::default(), None).await
            .context("Error while creating transaction state")?;
        
        // Create transaction with fee type
        let tx_version = storage.get_tx_version().await.context("Error while getting tx version")?;
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
            wallet.get_public_key().clone(),
            threshold,
            tx_type,
            fee_builder
        );
        
        // Set fee type if specified
        if let Some(ref ft) = fee_type {
            builder = builder.with_fee_type(ft.clone());
        }
        
        match builder.build(&mut state, wallet.get_keypair()) {
            Ok(tx) => {
                manager.message(&format!("Transaction created with {} fees", match fee_type.as_ref().unwrap_or(&tos_common::transaction::FeeType::TOS) {
                    tos_common::transaction::FeeType::TOS => "TOS",
                    tos_common::transaction::FeeType::Energy => "Energy",
                }));
                tx
            },
            Err(e) => {
                manager.error(&format!("Error while creating transaction: {}", e));
                return Ok(())
            }
        }
    };


    broadcast_tx(wallet, manager, tx).await;
    Ok(())
}

// Send the whole balance to a specified address
async fn transfer_all(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("transfer_all", &args)?;

    let prompt = manager.get_prompt();
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // read address
    let str_address = if args.has_argument("address") {
        args.get_value("address")?.to_string_value()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("address".to_string()));
    } else {
        prompt.read_input(
            prompt.colorize_string(Color::Green, "Address: "),
            false
        ).await.context("Error while reading address")?
    };
    let address = Address::from_string(&str_address).context("Invalid address")?;

    let asset = if args.has_argument("asset") {
        args.get_value("asset")?.to_hash()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("asset".to_string()));
    } else {
        let asset_opt = prompt.read_hash(
            prompt.colorize_string(Color::Green, "Asset (default TOS): ")
        ).await.ok();
        asset_opt.unwrap_or(TOS_ASSET)
    };
    
    // Read fee_type parameter
    let fee_type = if args.has_argument("fee_type") {
        let fee_type_str = args.get_value("fee_type")?.to_string_value()?;
        match fee_type_str.to_lowercase().as_str() {
            "tos" => Some(tos_common::transaction::FeeType::TOS),
            "energy" => Some(tos_common::transaction::FeeType::Energy),
            _ => {
                manager.error("Invalid fee_type. Use 'tos' or 'energy'");
                return Ok(());
            }
        }
    } else {
        None
    };

    // Validate fee_type for energy
    if fee_type.as_ref() == Some(&tos_common::transaction::FeeType::Energy) {
        if asset != TOS_ASSET {
            manager.error("Energy fees can only be used for TOS transfers");
            return Ok(());
        }
    }
    
    let (mut amount, asset_data, multisig) = {
        let storage = wallet.get_storage().read().await;
        let amount = storage.get_plaintext_balance_for(&asset).await.unwrap_or(0);
        let data = storage.get_asset(&asset).await?;
        let multisig = storage.get_multisig_state().await
            .context("Error while reading multisig state")?;
        (amount, data, multisig.cloned())
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
            wallet.estimate_fees(tx_type.clone(), FeeBuilder::default()).await.context("Error while estimating fees")?
        }
    } else {
        wallet.estimate_fees(tx_type.clone(), FeeBuilder::default()).await.context("Error while estimating fees")?
    };

    if asset == TOS_ASSET && fee_type.as_ref() != Some(&tos_common::transaction::FeeType::Energy) {
        amount = amount.checked_sub(estimated_fees).context("Insufficient balance to pay fees")?;
    }

    let fee_display = if let Some(ref ft) = fee_type {
        match ft {
            tos_common::transaction::FeeType::TOS => format!("TOS fees: {}", format_tos(estimated_fees)),
            tos_common::transaction::FeeType::Energy => "Energy fees: 0 TOS".to_string(),
        }
    } else {
        format!("TOS fees: {}", format_tos(estimated_fees))
    };
    
    manager.message(format!("Sending {} of {} ({}) to {} ({})", format_coin(amount, asset_data.get_decimals()), asset_data.get_name(), asset, address, fee_display));

    if !args.get_flag("confirm")? && !manager.is_batch_mode() && !prompt.ask_confirmation().await.context("Error while confirming action")? {
        manager.message("Transaction has been aborted");
        return Ok(())
    }

    manager.message("Building transaction...");
    let transfer = TransferBuilder {
        destination: address,
        amount,
        asset,
        extra_data: None,
    };
    let tx_type = TransactionTypeBuilder::Transfers(vec![transfer]);
    let tx = if let Some(multisig) = multisig {
        create_transaction_with_multisig(manager, prompt, wallet, tx_type, multisig.payload).await?
    } else {
        // Create transaction with appropriate fee type
        let storage = wallet.get_storage().read().await;
        let mut state = wallet.create_transaction_state_with_storage(&storage, &tx_type, &FeeBuilder::default(), None).await
            .context("Error while creating transaction state")?;
        
        // Create transaction with fee type
        let tx_version = storage.get_tx_version().await.context("Error while getting tx version")?;
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
            wallet.get_public_key().clone(),
            threshold,
            tx_type,
            fee_builder
        );
        
        // Set fee type if specified
        if let Some(ref ft) = fee_type {
            builder = builder.with_fee_type(ft.clone());
        }
        
        match builder.build(&mut state, wallet.get_keypair()) {
            Ok(tx) => {
                manager.message(&format!("Transaction created with {} fees", match fee_type.as_ref().unwrap_or(&tos_common::transaction::FeeType::TOS) {
                    tos_common::transaction::FeeType::TOS => "TOS",
                    tos_common::transaction::FeeType::Energy => "Energy",
                }));
                tx
            },
            Err(e) => {
                manager.error(&format!("Error while creating transaction: {}", e));
                return Ok(())
            }
        }
    };

    broadcast_tx(wallet, manager, tx).await;
    Ok(())
}

async fn burn(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("burn", &args)?;

    let prompt = manager.get_prompt();
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let asset = if args.has_argument("asset") {
        args.get_value("asset")?.to_hash()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("asset".to_string()));
    } else {
        prompt.read_hash(
            prompt.colorize_string(Color::Green, "Asset (default TOS): ")
        ).await.unwrap_or(TOS_ASSET)
    };

    let (max_balance, asset_data, multisig) = {
        let storage = wallet.get_storage().read().await;
        let balance = storage.get_plaintext_balance_for(&asset).await.unwrap_or(0);
        let data = storage.get_asset(&asset).await?;
        let multisig = storage.get_multisig_state().await
            .context("Error while reading multisig state")?;
        (balance, data, multisig.cloned())
    };

    // read amount
    let amount = if args.has_argument("amount") {
        args.get_value("amount")?.to_string_value()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("amount".to_string()));
    } else {
        prompt.read(
            prompt.colorize_string(Color::Green, &format!("Amount (max: {}): ", format_coin(max_balance, asset_data.get_decimals())))
        ).await.context("Error while reading amount")?
    };

    let amount = from_coin(amount, asset_data.get_decimals()).context("Invalid amount")?;
    manager.message(format!("Burning {} of {} ({})", format_coin(amount, asset_data.get_decimals()), asset_data.get_name(), asset));
    if !args.get_flag("confirm")? && !manager.is_batch_mode() && !prompt.ask_confirmation().await.context("Error while confirming action")? {
        manager.message("Transaction has been aborted");
        return Ok(())
    }

    manager.message("Building transaction...");
    let payload = BurnPayload {
        amount,
        asset
    };

    let tx_type = TransactionTypeBuilder::Burn(payload);
    let tx = if let Some(multisig) = multisig {
        create_transaction_with_multisig(manager, prompt, wallet, tx_type, multisig.payload).await?
    } else {
        match wallet.create_transaction(tx_type, FeeBuilder::default()).await {
            Ok(tx) => tx,
            Err(e) => {
                manager.error(&format!("Error while creating transaction: {}", e));
                return Ok(())
            }
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
async fn balance(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let prompt = manager.get_prompt();
    let wallet: &Arc<Wallet> = context.get()?;
    let storage = wallet.get_storage().read().await;

    let asset = if arguments.has_argument("asset") {
        arguments.get_value("asset")?.to_hash()?
    } else if manager.is_batch_mode() {
        TOS_ASSET  // Default to TOS in batch mode
    } else {
        prompt.read_hash(
            prompt.colorize_string(Color::Green, "Asset (default TOS): ")
        ).await.unwrap_or(TOS_ASSET)
    };
    let balance = storage.get_plaintext_balance_for(&asset).await?;
    let data = storage.get_asset(&asset).await?;
    manager.message(format!("Balance for asset {} ({}): {}", data.get_name(), asset, format_coin(balance, data.get_decimals())));
    Ok(())
}

// Show all transactions
async fn history(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let page = if arguments.has_argument("page") {
        arguments.get_value("page")?.to_number()? as usize
    } else {
        1
    };

    if page == 0 {
        return Err(CommandError::InvalidArgument("Page must be greater than 0".to_string()));
    }

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let storage = wallet.get_storage().read().await;

    let txs_len = storage.get_transactions_count()?;
    // if we don't have any txs, no need proceed further
    if txs_len == 0 {
        manager.message("No transactions available");
        return Ok(())
    }

    let mut max_pages = txs_len / ELEMENTS_PER_PAGE;
    if txs_len % ELEMENTS_PER_PAGE != 0 {
        max_pages += 1;
    }

    if page > max_pages {
        return Err(CommandError::InvalidArgument(format!("Page must be less than maximum pages ({})", max_pages)));
    }

    let transactions = storage.get_filtered_transactions(
        None,
        None,
        None,
        None,
        true,
        true,
        true,
        true,
        None,
        Some(ELEMENTS_PER_PAGE),
        Some((page - 1) * ELEMENTS_PER_PAGE)
    )?;

    manager.message(format!("{} Transactions (total {}) page {}/{}:", transactions.len(), txs_len, page, max_pages));
    for tx in transactions {
        manager.message(format!("- {}", tx.summary(wallet.get_network().is_mainnet(), &*storage).await?));
    }

    Ok(())
}

async fn transaction(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let storage = wallet.get_storage().read().await;
    let hash = arguments.get_value("hash")?.to_hash()?;
    let tx = storage.get_transaction(&hash).context("Transaction not found")?;
    manager.message(tx.summary(wallet.get_network().is_mainnet(), &*storage).await?);
    Ok(())
}

async fn export_transactions_csv(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let filename = arguments.get_value("filename")?.to_string_value()?;
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let storage = wallet.get_storage().read().await;
    let transactions = storage.get_transactions()?;
    let mut file = File::create(&filename).context("Error while creating CSV file")?;

    wallet.export_transactions_in_csv(&storage, transactions, &mut file).await.context("Error while exporting transactions to CSV")?;

    manager.message(format!("Transactions have been exported to {}", filename));
    Ok(())
}

async fn clear_tx_cache(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let mut storage = wallet.get_storage().write().await;
    storage.clear_tx_cache();
    manager.message("Transaction cache has been cleared");
    Ok(())
}

// Set your wallet in online mode
#[cfg(feature = "network_handler")]
async fn online_mode(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    if wallet.is_online().await {
        manager.error("Wallet is already online");
    } else {
        let daemon_address = if arguments.has_argument("daemon_address") {
            arguments.get_value("daemon_address")?.to_string_value()?
        } else {
            DEFAULT_DAEMON_ADDRESS.to_string()
        };

        wallet.set_online_mode(&daemon_address, true).await.context("Couldn't enable online mode")?;
        manager.message("Wallet is now online");
    }
    Ok(())
}

// Set your wallet in offline mode
#[cfg(feature = "network_handler")]
async fn offline_mode(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    if !wallet.is_online().await {
        manager.error("Wallet is already offline");
    } else {
        wallet.set_offline_mode().await.context("Error on offline mode")?;
        manager.message("Wallet is now offline");
    }
    Ok(())
}

// Show current wallet address
#[cfg(feature = "network_handler")]
async fn rescan(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let topoheight = if arguments.has_argument("topoheight") {
        arguments.get_value("topoheight")?.to_number()?
    } else {
        0
    };

    if let Err(e) = wallet.rescan(topoheight, true).await {
        manager.error(format!("Error while rescanning: {:#}", e));
    } else {
        manager.message("Network handler has been restarted!");
    }
    Ok(())
}

async fn seed(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("seed", &arguments)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let prompt = manager.get_prompt();

    let password = if arguments.has_argument("password") {
        arguments.get_value("password")?.to_string_value()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("password".to_string()));
    } else {
        prompt.read_input("Password: ", true)
            .await.context("Error while reading password")?
    };

    // check if password is valid
    wallet.is_valid_password(&password).await?;

    let language = if arguments.has_argument("language") {
        arguments.get_value("language")?.to_number()?
    } else {
        0
    };

    let seed = wallet.get_seed(language as usize)?;
    if manager.is_batch_mode() {
        manager.message(format!("Seed: {}", seed));
    } else {
        prompt.read_input(
            prompt.colorize_string(Color::Green, &format!("Seed: {}\r\nPress ENTER to continue", seed)),
            false
        ).await.context("Error while printing seed")?;
    }
    Ok(())
}

async fn nonce(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let storage = wallet.get_storage().read().await;
    let nonce = storage.get_nonce()?;
    let unconfirmed_nonce = storage.get_unconfirmed_nonce()?;
    manager.message(format!("Nonce: {}", nonce));
    if nonce != unconfirmed_nonce {
        manager.message(format!("Unconfirmed nonce: {}", unconfirmed_nonce));
    }

    Ok(())
}

async fn set_nonce(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("set_nonce", &args)?;

    let value = if args.has_argument("nonce") {
        args.get_value("nonce")?.to_number()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("nonce".to_string()));
    } else {
        manager.get_prompt().read("New Nonce: ".to_string()).await
            .context("Error while reading new nonce to set")?
    };

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let mut storage = wallet.get_storage().write().await;
    storage.set_nonce(value)?;
    storage.clear_tx_cache();

    manager.message(format!("New nonce is: {}", value));
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

async fn set_tx_version(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    let value: u8 = if args.has_argument("version") {
        args.get_value("version")?.to_number()?.try_into()
            .map_err(|_| CommandError::InvalidArgument("Invalid transaction version".to_string()))?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("version".to_string()));
    } else {
        manager.get_prompt().read("New Transaction Version: ".to_string()).await
            .context("Error while reading new transaction version to set")?
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
            let info = api.get_info().await
                .context("Error while getting network info")?;

            manager.message("--- Daemon status ---");
            manager.message(format!("Blue Score: {}", info.blue_score));
            manager.message(format!("Topoheight: {}", info.topoheight));
            manager.message(format!("Stable Blue Score: {}", info.stable_blue_score));
            manager.message(format!("Pruned topoheight: {:?}", info.pruned_topoheight));
            manager.message(format!("Top block hash: {}", info.top_block_hash));
            manager.message(format!("Network: {}", info.network));
            manager.message(format!("Emitted supply: {}", format_tos(info.emitted_supply)));
            manager.message(format!("Burned supply: {}", format_tos(info.burned_supply)));
            manager.message(format!("Circulating supply: {}", format_tos(info.circulating_supply)));
            manager.message("---------------------");
        }
    }

    let storage = wallet.get_storage().read().await;
    let multisig = storage.get_multisig_state().await?;
    if let Some(multisig) = multisig {
        manager.message("--- Multisig: ---");
        manager.message(format!("Threshold: {}", multisig.payload.threshold));
        manager.message(format!("Participants ({}): {}", multisig.payload.participants.len(),
            multisig.payload.participants.iter()
                .map(|p| p.as_address(wallet.get_network().is_mainnet()).to_string())
                .collect::<Vec<_>>().join(", ")
            ));
        manager.message("---------------");
    } else {
        manager.message("No multisig state");
    }

    let tx_version = storage.get_tx_version().await?;
    manager.message(format!("Transaction version: {}", tx_version));
    let nonce = storage.get_nonce()?;
    let unconfirmed_nonce = storage.get_unconfirmed_nonce()?;
    manager.message(format!("Nonce: {}", nonce));
    if nonce != unconfirmed_nonce {
        manager.message(format!("Unconfirmed nonce: {}", unconfirmed_nonce));
    }
    let network = wallet.get_network();
    manager.message(format!("Synced topoheight: {}", storage.get_synced_topoheight()?));
    manager.message(format!("Network: {}", network));
    manager.message(format!("Wallet address: {}", wallet.get_address()));

    Ok(())
}

// Show AI mining transaction history
async fn ai_mining_history(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let page = if arguments.has_argument("page") {
        arguments.get_value("page")?.to_number()? as usize
    } else {
        0
    };

    let limit = if arguments.has_argument("limit") {
        arguments.get_value("limit")?.to_number()? as usize
    } else {
        20
    };

    let filter_type = if arguments.has_argument("type") {
        Some(arguments.get_value("type")?.to_string_value()?)
    } else {
        None
    };

    let storage = wallet.get_storage().read().await;
    let all_transactions = storage.get_transactions()?;

    // Filter for AI mining transactions
    let ai_transactions: Vec<_> = all_transactions.iter()
        .filter(|tx| {
            if let EntryData::AIMining { payload, .. } = tx.get_entry() {
                if let Some(ref filter) = filter_type {
                    match filter.to_lowercase().as_str() {
                        "publish" | "publishtask" => matches!(payload, AIMiningPayload::PublishTask { .. }),
                        "submit" | "submitanswer" => matches!(payload, AIMiningPayload::SubmitAnswer { .. }),
                        "validate" | "validateanswer" => matches!(payload, AIMiningPayload::ValidateAnswer { .. }),
                        "register" | "registerminer" => matches!(payload, AIMiningPayload::RegisterMiner { .. }),
                        _ => true
                    }
                } else {
                    true
                }
            } else {
                false
            }
        })
        .collect();

    if ai_transactions.is_empty() {
        manager.message("No AI mining transactions found");
        return Ok(());
    }

    let total_count = ai_transactions.len();
    let start_idx = page * limit;
    let end_idx = std::cmp::min(start_idx + limit, total_count);

    if start_idx >= total_count {
        manager.message("No AI mining transactions found on this page");
        return Ok(());
    }

    let type_filter_str = filter_type.map(|t| format!(" (filtered by {})", t)).unwrap_or_default();
    manager.message(format!("AI Mining Transaction History{} (page {}, showing {}-{} of {})",
        type_filter_str, page, start_idx + 1, end_idx, total_count));
    manager.message("=".repeat(80));

    let network = wallet.get_network();
    for tx in &ai_transactions[start_idx..end_idx] {
        let summary = tx.summary(network.is_mainnet(), &storage).await?;
        manager.message(summary);
    }

    if end_idx < total_count {
        manager.message(format!("Use 'ai_mining_history --page {}' to see more transactions", page + 1));
    }

    Ok(())
}

// Show AI mining statistics for this wallet
async fn ai_mining_stats(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let storage = wallet.get_storage().read().await;
    let all_transactions = storage.get_transactions()?;
    let network = wallet.get_network();
    let wallet_address = wallet.get_address();

    let mut stats = AIMiningSummary::default();

    // Analyze all AI mining transactions
    for tx in &all_transactions {
        if let EntryData::AIMining { payload, outgoing, .. } = tx.get_entry() {
            match payload {
                AIMiningPayload::PublishTask { reward_amount, difficulty, .. } => {
                    if *outgoing {
                        stats.tasks_published += 1;
                        stats.total_rewards_offered += reward_amount;
                        stats.difficulty_breakdown.entry(format!("{:?}", difficulty)).or_insert(0).add_assign(1);
                    }
                },
                AIMiningPayload::SubmitAnswer { stake_amount, .. } => {
                    if *outgoing {
                        stats.answers_submitted += 1;
                        stats.total_staked += stake_amount;
                    }
                },
                AIMiningPayload::ValidateAnswer { validation_score, .. } => {
                    if *outgoing {
                        stats.validations_performed += 1;
                        stats.total_validation_score += *validation_score as u64;
                    }
                },
                AIMiningPayload::RegisterMiner { registration_fee, .. } => {
                    if *outgoing {
                        stats.registrations += 1;
                        stats.total_registration_fees += registration_fee;
                    }
                },
            }
        }
    }

    manager.message("=== AI Mining Statistics ===");
    manager.message(format!("Wallet Address: {}", wallet_address));
    manager.message(format!("Network: {}", network));
    manager.message("");

    manager.message("--- Activity Summary ---");
    manager.message(format!("Tasks Published: {}", stats.tasks_published));
    manager.message(format!("Answers Submitted: {}", stats.answers_submitted));
    manager.message(format!("Validations Performed: {}", stats.validations_performed));
    manager.message(format!("Miner Registrations: {}", stats.registrations));
    manager.message("");

    manager.message("--- Financial Summary ---");
    manager.message(format!("Total Rewards Offered: {} TOS", format_tos(stats.total_rewards_offered)));
    manager.message(format!("Total Amount Staked: {} TOS", format_tos(stats.total_staked)));
    manager.message(format!("Total Registration Fees: {} TOS", format_tos(stats.total_registration_fees)));
    if stats.validations_performed > 0 {
        let avg_score = stats.total_validation_score as f64 / stats.validations_performed as f64;
        manager.message(format!("Average Validation Score: {:.1}", avg_score));
    }
    manager.message("");

    if !stats.difficulty_breakdown.is_empty() {
        manager.message("--- Task Difficulty Breakdown ---");
        for (difficulty, count) in stats.difficulty_breakdown {
            manager.message(format!("{}: {} tasks", difficulty, count));
        }
    }

    Ok(())
}

// Show AI mining tasks this wallet has interacted with
async fn ai_mining_tasks(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let page = if arguments.has_argument("page") {
        arguments.get_value("page")?.to_number()? as usize
    } else {
        0
    };

    let status_filter = if arguments.has_argument("status") {
        Some(arguments.get_value("status")?.to_string_value()?)
    } else {
        None
    };

    // Show AI mining transactions from local history
    let storage = wallet.get_storage().read().await;
    let all_transactions = storage.get_transactions()?;

    // Filter for AI mining transactions
    let mut ai_transactions: Vec<_> = all_transactions.iter()
        .filter(|tx| matches!(tx.get_entry(), EntryData::AIMining { .. }))
        .collect();

    ai_transactions.sort_by_key(|tx| std::cmp::Reverse(tx.get_topoheight()));

    let total_count = ai_transactions.len();
    let start_idx = page * 10;
    let end_idx = std::cmp::min(start_idx + 10, total_count);

    if total_count == 0 {
        manager.message("No AI mining transactions found in wallet history");
        return Ok(());
    }

    if start_idx >= total_count {
        manager.message("No transactions found on this page");
        return Ok(());
    }

    let status_filter_str = status_filter.map(|s| format!(" (filter: {})", s)).unwrap_or_default();
    manager.message(format!("AI Mining Transaction History{} (page {}, showing {}-{} of {})",
        status_filter_str, page, start_idx + 1, end_idx, total_count));
    manager.message("=".repeat(80));

    for tx in &ai_transactions[start_idx..end_idx] {
        if let EntryData::AIMining { payload, outgoing, .. } = tx.get_entry() {
            let direction = if *outgoing { "OUTGOING" } else { "INCOMING" };

            manager.message(format!("[{}] {}", direction, tx.get_hash()));
            manager.message(format!("  TopoHeight: {}", tx.get_topoheight()));

            match payload {
                AIMiningPayload::PublishTask { task_id, reward_amount, difficulty, .. } => {
                    manager.message(format!("  Type: Publish Task"));
                    manager.message(format!("  Task ID: {}", task_id));
                    manager.message(format!("  Reward: {} TOS", format_tos(*reward_amount)));
                    manager.message(format!("  Difficulty: {:?}", difficulty));
                },
                AIMiningPayload::SubmitAnswer { task_id, answer_hash, stake_amount, answer_content: _ } => {
                    manager.message(format!("  Type: Submit Answer"));
                    manager.message(format!("  Task ID: {}", task_id));
                    manager.message(format!("  Answer Hash: {}", answer_hash));
                    manager.message(format!("  Stake: {} TOS", format_tos(*stake_amount)));
                },
                AIMiningPayload::ValidateAnswer { task_id, answer_id, validation_score } => {
                    manager.message(format!("  Type: Validate Answer"));
                    manager.message(format!("  Task ID: {}", task_id));
                    manager.message(format!("  Answer ID: {}", answer_id));
                    manager.message(format!("  Validation Score: {}", validation_score));
                },
                AIMiningPayload::RegisterMiner { miner_address, registration_fee } => {
                    manager.message(format!("  Type: Register Miner"));
                    manager.message(format!("  Miner Address: {}", miner_address.as_address(wallet.get_network().is_mainnet())));
                    manager.message(format!("  Registration Fee: {} TOS", format_tos(*registration_fee)));
                },
            }
            manager.message("");
        }
    }

    if end_idx < total_count {
        manager.message(format!("Use 'ai_mining_tasks --page {}' to see more transactions", page + 1));
    }

    Ok(())
}

// Show AI mining rewards earned
async fn ai_mining_rewards(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let page = if arguments.has_argument("page") {
        arguments.get_value("page")?.to_number()? as usize
    } else {
        0
    };

    let storage = wallet.get_storage().read().await;
    let all_transactions = storage.get_transactions()?;

    // Find incoming transactions that could be rewards
    let potential_rewards: Vec<_> = all_transactions.iter()
        .filter(|tx| {
            match tx.get_entry() {
                EntryData::Incoming { transfers, .. } => {
                    // Look for TOS transfers that could be AI mining rewards
                    transfers.iter().any(|transfer| transfer.get_asset() == &TOS_ASSET)
                },
                EntryData::AIMining { outgoing, .. } => !outgoing, // Incoming AI mining transactions
                _ => false
            }
        })
        .collect();

    if potential_rewards.is_empty() {
        manager.message("No potential AI mining rewards found");
        return Ok(());
    }

    let total_count = potential_rewards.len();
    let start_idx = page * 20;
    let end_idx = std::cmp::min(start_idx + 20, total_count);

    if start_idx >= total_count {
        manager.message("No rewards found on this page");
        return Ok(());
    }

    manager.message(format!("Potential AI Mining Rewards (page {}, showing {}-{} of {})",
        page, start_idx + 1, end_idx, total_count));
    manager.message("Note: This includes all incoming TOS transfers and AI mining transactions");
    manager.message("=".repeat(80));

    let network = wallet.get_network();
    let mut total_rewards = 0u64;

    for tx in &potential_rewards[start_idx..end_idx] {
        let summary = tx.summary(network.is_mainnet(), &storage).await?;
        manager.message(summary);

        // Try to extract reward amounts
        match tx.get_entry() {
            EntryData::Incoming { transfers, .. } => {
                for transfer in transfers {
                    if transfer.get_asset() == &TOS_ASSET {
                        total_rewards += transfer.get_amount();
                    }
                }
            },
            _ => {}
        }
    }

    if total_rewards > 0 {
        manager.message("");
        manager.message(format!("Total TOS received in this page: {} TOS", format_tos(total_rewards)));
    }

    if end_idx < total_count {
        manager.message(format!("Use 'ai_mining_rewards --page {}' to see more rewards", page + 1));
    }

    Ok(())
}

// AI Mining business commands implementation
async fn publish_task(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let wallet = {
        let context = manager.get_context().lock()?;
        context.get::<Arc<Wallet>>()?.clone()
    };

    let description = arguments.get_value("description")?.to_string_value()?;
    let reward = arguments.get_value("reward")?.to_number()? as u64;
    let difficulty_str = arguments.get_value("difficulty")?.to_string_value()?;
    let deadline = arguments.get_value("deadline")?.to_number()? as u64;

    // Parse difficulty level
    let difficulty = match difficulty_str.to_lowercase().as_str() {
        "beginner" => DifficultyLevel::Beginner,
        "intermediate" => DifficultyLevel::Intermediate,
        "advanced" => DifficultyLevel::Advanced,
        "expert" => DifficultyLevel::Expert,
        _ => return Err(CommandError::InvalidArgument("difficulty must be: beginner, intermediate, advanced, or expert".to_string())),
    };

    // Convert reward from TOS to nanoTOS
    let reward_nanos = reward * 1_000_000_000;

    // Validate reward is within difficulty range
    let (min_reward, max_reward) = difficulty.reward_range();
    if reward_nanos < min_reward || reward_nanos > max_reward {
        return Err(CommandError::InvalidArgument(format!(
            "Reward {} TOS is outside valid range [{}, {}] TOS for difficulty {:?}",
            reward, min_reward / 1_000_000_000, max_reward / 1_000_000_000, difficulty
        )));
    }

    // Generate task ID from description and current time
    let task_data = format!("{}-{}-{}", description, reward_nanos, deadline);
    let task_id_bytes = Keccak256::digest(task_data.as_bytes());
    let task_id = Hash::from_hex(&hex::encode(task_id_bytes)).unwrap_or_else(|_| Hash::zero());

    // Create AI mining payload
    let ai_mining_payload = AIMiningPayload::PublishTask {
        task_id: task_id.clone(),
        reward_amount: reward_nanos,
        difficulty: difficulty.clone(),
        deadline,
        description: description.clone(),
    };

    // Validate payload before creating transaction
    ai_mining_payload.validate().map_err(|e| CommandError::InvalidArgument(e.to_string()))?;

    // Create transaction type
    let tx_type = TransactionTypeBuilder::AIMining(ai_mining_payload);

    manager.message(format!("Publishing AI mining task:"));
    manager.message(format!("  Task ID: {}", task_id));
    manager.message(format!("  Description: {}", description));
    manager.message(format!("  Reward: {} TOS", reward));
    manager.message(format!("  Difficulty: {:?}", difficulty));
    manager.message(format!("  Deadline: {}", deadline));

    // Build and submit transaction
    match wallet.create_transaction(tx_type, FeeBuilder::default()).await {
        Ok(tx) => {
            manager.message(format!("Transaction created successfully: {}", tx.hash()));
            manager.message("AI mining task published!");
        }
        Err(e) => {
            return Err(CommandError::InvalidArgument(format!("Failed to create transaction: {}", e)));
        }
    }

    Ok(())
}

async fn submit_answer(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let wallet = {
        let context = manager.get_context().lock()?;
        context.get::<Arc<Wallet>>()?.clone()
    };

    let task_id_str = arguments.get_value("task_id")?.to_string_value()?;
    let answer_content = arguments.get_value("answer_content")?.to_string_value()?;
    let answer_hash_str = arguments.get_value("answer_hash")?.to_string_value()?;
    let stake = arguments.get_value("stake")?.to_number()? as u64;

    // Parse hashes
    let task_id = Hash::from_hex(&task_id_str).map_err(|_|
        CommandError::InvalidArgument("Invalid task_id format".to_string()))?;
    let answer_hash = Hash::from_hex(&answer_hash_str).map_err(|_|
        CommandError::InvalidArgument("Invalid answer_hash format".to_string()))?;

    // Convert stake from TOS to nanoTOS
    let stake_nanos = stake * 1_000_000_000;

    // Create AI mining payload
    let ai_mining_payload = AIMiningPayload::SubmitAnswer {
        task_id: task_id.clone(),
        answer_content: answer_content.clone(),
        answer_hash: answer_hash.clone(),
        stake_amount: stake_nanos,
    };

    // Validate payload before creating transaction
    ai_mining_payload.validate().map_err(|e| CommandError::InvalidArgument(e.to_string()))?;

    // Create transaction type
    let tx_type = TransactionTypeBuilder::AIMining(ai_mining_payload);

    manager.message(format!("Submitting answer to AI mining task:"));
    manager.message(format!("  Task ID: {}", task_id));
    manager.message(format!("  Answer Hash: {}", answer_hash));
    manager.message(format!("  Stake: {} TOS", stake));

    // Build and submit transaction
    match wallet.create_transaction(tx_type, FeeBuilder::default()).await {
        Ok(tx) => {
            manager.message(format!("Transaction created successfully: {}", tx.hash()));
            manager.message("Answer submitted to AI mining task!");
        }
        Err(e) => {
            return Err(CommandError::InvalidArgument(format!("Failed to create transaction: {}", e)));
        }
    }

    Ok(())
}

async fn validate_answer(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let wallet = {
        let context = manager.get_context().lock()?;
        context.get::<Arc<Wallet>>()?.clone()
    };

    let task_id_str = arguments.get_value("task_id")?.to_string_value()?;
    let answer_id_str = arguments.get_value("answer_id")?.to_string_value()?;
    let score = arguments.get_value("score")?.to_number()? as u8;

    // Validate score range
    if score > 100 {
        return Err(CommandError::InvalidArgument("Score must be between 0 and 100".to_string()));
    }

    // Parse hashes
    let task_id = Hash::from_hex(&task_id_str).map_err(|_|
        CommandError::InvalidArgument("Invalid task_id format".to_string()))?;
    let answer_id = Hash::from_hex(&answer_id_str).map_err(|_|
        CommandError::InvalidArgument("Invalid answer_id format".to_string()))?;

    // Create AI mining payload
    let ai_mining_payload = AIMiningPayload::ValidateAnswer {
        task_id: task_id.clone(),
        answer_id: answer_id.clone(),
        validation_score: score,
    };

    // Create transaction type
    let tx_type = TransactionTypeBuilder::AIMining(ai_mining_payload);

    manager.message(format!("Validating answer for AI mining task:"));
    manager.message(format!("  Task ID: {}", task_id));
    manager.message(format!("  Answer ID: {}", answer_id));
    manager.message(format!("  Score: {}/100", score));

    // Build and submit transaction
    match wallet.create_transaction(tx_type, FeeBuilder::default()).await {
        Ok(tx) => {
            manager.message(format!("Transaction created successfully: {}", tx.hash()));
            manager.message("Answer validation submitted!");
        }
        Err(e) => {
            return Err(CommandError::InvalidArgument(format!("Failed to create transaction: {}", e)));
        }
    }

    Ok(())
}

async fn register_miner(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    let (wallet, wallet_address) = {
        let context = manager.get_context().lock()?;
        let wallet = context.get::<Arc<Wallet>>()?.clone();
        let wallet_address = wallet.get_address().clone();
        (wallet, wallet_address)
    };

    let fee = arguments.get_value("fee")?.to_number()? as u64;

    // Convert fee from TOS to nanoTOS
    let fee_nanos = fee * 1_000_000_000;

    // Create AI mining payload
    let ai_mining_payload = AIMiningPayload::RegisterMiner {
        miner_address: wallet_address.get_public_key().clone(),
        registration_fee: fee_nanos,
    };

    // Create transaction type
    let tx_type = TransactionTypeBuilder::AIMining(ai_mining_payload);

    manager.message(format!("Registering as AI miner:"));
    manager.message(format!("  Miner Address: {}", wallet_address));
    manager.message(format!("  Registration Fee: {} TOS", fee));

    // Build and submit transaction
    match wallet.create_transaction(tx_type, FeeBuilder::default()).await {
        Ok(tx) => {
            manager.message(format!("Transaction created successfully: {}", tx.hash()));
            manager.message("AI miner registration submitted!");
        }
        Err(e) => {
            return Err(CommandError::InvalidArgument(format!("Failed to create transaction: {}", e)));
        }
    }

    Ok(())
}

#[derive(Default)]
struct AIMiningSummary {
    tasks_published: u32,
    answers_submitted: u32,
    validations_performed: u32,
    registrations: u32,
    total_rewards_offered: u64,
    total_staked: u64,
    total_registration_fees: u64,
    total_validation_score: u64,
    difficulty_breakdown: std::collections::HashMap<String, u32>,
}

async fn logout(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    {
        let context = manager.get_context().lock()?;
        let wallet: &Arc<Wallet> = context.get()?;
        wallet.close().await;
    }

    manager.remove_all_commands().context("Error while removing all commands")?;
    manager.remove_from_context::<Arc<Wallet>>()?;

    register_default_commands(manager).await?;
    manager.message("Wallet has been closed");

    Ok(())
}

#[cfg(feature = "api_server")]
async fn stop_api_server(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    wallet.stop_api_server().await.context("Error while stopping API Server")?;
    manager.message("API Server has been stopped");
    Ok(())
}

#[cfg(feature = "api_server")]
async fn start_rpc_server(manager: &CommandManager, mut arguments: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("start_rpc_server", &arguments)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let bind_address = arguments.get_value("bind_address")?.to_string_value()?;
    let username = arguments.get_value("username")?.to_string_value()?;
    let password = arguments.get_value("password")?.to_string_value()?;

    let auth_config = Some(AuthConfig {
        username,
        password
    });

    wallet.enable_rpc_server(bind_address, auth_config, None).await.context("Error while enabling RPC Server")?;
    manager.message("RPC Server has been enabled");
    Ok(())
}

#[cfg(feature = "api_server")]
async fn start_xswd(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    match wallet.enable_xswd().await {
        Ok(receiver) => {
            if let Some(receiver) = receiver {
                let prompt = manager.get_prompt().clone();
                spawn_task("xswd", xswd_handler(receiver, prompt));
            }

            manager.message("XSWD Server has been enabled");
        },
        Err(e) => manager.error(format!("Error while enabling XSWD Server: {}", e))
    };

    Ok(())
}


#[cfg(feature = "xswd")]
async fn add_xswd_relayer(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    let app_data = if args.has_argument("app_data") {
        args.get_value("app_data")?.to_string_value()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("app_data".to_string()));
    } else {
        manager.get_prompt()
            .read("App data").await
            .context("Error while reading app data")?
    };

    let app_data = serde_json::from_str(&app_data)
        .context("Error while parsing app data as JSON")?;

    match wallet.add_xswd_relayer(app_data).await {
        Ok(receiver) => {
            if let Some(receiver) = receiver {
                let prompt = manager.get_prompt().clone();
                spawn_task("xswd", xswd_handler(receiver, prompt));
            }

            manager.message("XSWD Server has been enabled");
        },
        Err(e) => manager.error(format!("Error while enabling XSWD Server: {}", e))
    };

    Ok(())
}

// Setup a multisig transaction (a multisig is present on chain, but this wallet is offline so can't sync it)
async fn multisig_setup(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let prompt = manager.get_prompt();

    let multisig = {
        let storage = wallet.get_storage().read().await;
        storage.get_multisig_state().await?.cloned()
    };

    if !manager.is_batch_mode() {
        manager.warn("IMPORTANT: Make sure you have the correct participants and threshold before proceeding.");
        manager.warn("If you are unsure, please cancel and verify the participants and threshold.");
        manager.warn("An incorrect setup can lead to loss of funds.");
        manager.warn("Do you want to continue?");

        if !prompt.ask_confirmation().await.context("Error while confirming action")? {
            manager.message("Transaction has been aborted");
            return Ok(())
        }
    }

    let participants: u8 = if args.has_argument("participants") {
        args.get_value("participants")?.to_number()? as u8
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("participants".to_string()));
    } else {
        let msg = if multisig.is_some() {
            "Participants count (0 to delete): "
        } else {
            "Participants count (min. 1): "
        };
        prompt.read(msg)
            .await.context("Error while reading participants count")?
    };

    if participants == 0 {
        let Some(multisig) = multisig else {
            return Err(CommandError::InvalidArgument("Participants count must be greater than 0".to_string()));
        };

        if !manager.is_batch_mode() {
            manager.warn("Participants count is 0, this will delete the multisig currently configured");
            manager.warn("Do you want to continue?");
        }

        if !args.get_flag("confirm")? && !manager.is_batch_mode() && !prompt.ask_confirmation().await.context("Error while confirming action")? {
            manager.message("Transaction has been aborted");
            return Ok(())
        }

        let payload = MultiSigBuilder {
            participants: IndexSet::new(),
            threshold: 0
        };

        let tx = create_transaction_with_multisig(manager, prompt, wallet, TransactionTypeBuilder::MultiSig(payload), multisig.payload).await?;

        broadcast_tx(wallet, manager, tx).await;
        return Ok(())
    }

    let threshold: u8 = if args.has_argument("threshold") {
        args.get_value("threshold")?.to_number()? as u8
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("threshold".to_string()));
    } else {
        prompt.read("Threshold (min. 1): ")
            .await.context("Error while reading threshold")?
    };

    if threshold == 0 {
        return Err(CommandError::InvalidArgument("Threshold must be greater than 0".to_string()));
    }

    if threshold > participants {
        return Err(CommandError::InvalidArgument("Threshold must be less or equal to participants count".to_string()));
    }

    if manager.is_batch_mode() {
        return Err(CommandError::BatchModeError("multisig_setup command requires interactive mode to collect participant addresses".to_string()));
    }

    let mainnet = wallet.get_network().is_mainnet();
    let mut keys = IndexSet::with_capacity(participants as usize);
    for i in 0..participants {
        let address: Address = prompt.read(format!("Participant #{} address: ", i + 1))
            .await.context("Error while reading participant address")?;

        if address.is_mainnet() != mainnet {
            return Err(CommandError::InvalidArgument("Participant address must be on the same network".to_string()));
        }

        if !address.is_normal() {
            return Err(CommandError::InvalidArgument("Participant address must be a normal address".to_string()));
        }

        if address.get_public_key() == wallet.get_public_key() {
            return Err(CommandError::InvalidArgument("Participant address cannot be the same as the wallet address".to_string()));
        }

        if !keys.insert(address) {
            return Err(CommandError::InvalidArgument("Participant address already exists".to_string()));
        }
    }

    manager.message(format!("MultiSig payload ({} participants with threshold at {}):", participants, threshold));
    for key in keys.iter() {
        manager.message(format!("- {}", key));
    }

    if !args.get_flag("confirm")? && !manager.is_batch_mode() && !prompt.ask_confirmation().await.context("Error while confirming action")? {
        manager.message("Transaction has been aborted");
        return Ok(())
    }

    manager.message("Building transaction...");

    let multisig = {
        let storage = wallet.get_storage().read().await;
        storage.get_multisig_state().await.context("Error while reading multisig state")?
            .cloned()
    };
    let payload = MultiSigBuilder {
        participants: keys,
        threshold
    };
    let tx_type = TransactionTypeBuilder::MultiSig(payload);
    let tx = if let Some(multisig) = multisig {
        create_transaction_with_multisig(manager, prompt, wallet, tx_type, multisig.payload).await?
    } else {
        match wallet.create_transaction(tx_type, FeeBuilder::default()).await {
            Ok(tx) => tx,
            Err(e) => {
                manager.error(&format!("Error while creating transaction: {}", e));
                return Ok(())
            }
        }
    };

    broadcast_tx(wallet, manager, tx).await;

    Ok(())
}

async fn multisig_sign(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("multisig_sign", &args)?;

    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let prompt = manager.get_prompt();

    let tx_hash = if args.has_argument("tx_hash") {
        args.get_value("tx_hash")?.to_hash()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("tx_hash".to_string()));
    } else {
        prompt.read("Transaction hash: ").await.context("Error while reading transaction hash")?
    };

    let signature = wallet.sign_data(tx_hash.as_bytes());
    if manager.is_batch_mode() {
        manager.message(format!("Signature: {}", signature.to_hex()));
    } else {
        prompt.read_input(format!("Signature: {}\r\nPress ENTER to continue", signature.to_hex()), false).await
            .context("Error while displaying signature")?;
    }

    Ok(())
}

async fn multisig_show(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    let storage = wallet.get_storage().read().await;
    let multisig = storage.get_multisig_state().await.context("Error while reading multisig state")?;

    if let Some(multisig) = multisig {
        manager.message(format!("MultiSig payload ({} participants with threshold at {}):", multisig.payload.participants.len(), multisig.payload.threshold));
        for key in multisig.payload.participants.iter() {
            manager.message(format!("- {}", key.as_address(wallet.get_network().is_mainnet())));
        }
    } else {
        manager.message("No multisig configured");
    }

    Ok(())
}

// broadcast tx if possible
// submit_transaction increase the local nonce in storage in case of success
async fn broadcast_tx(wallet: &Wallet, manager: &CommandManager, tx: Transaction) {
    let tx_hash = tx.hash();
    manager.message(format!("Transaction hash: {}", tx_hash));

    if wallet.is_online().await {
        if let Err(e) = wallet.submit_transaction(&tx).await {
            let error_msg = format!("{:#}", e);

            // Check if error is due to nonce conflict
            if error_msg.contains("nonce") && error_msg.contains("already used") {
                if log::log_enabled!(log::Level::Info) {
                    info!("Detected nonce conflict, attempting to sync nonce from blockchain");
                }
                manager.warn("Nonce conflict detected. Attempting to sync nonce from blockchain...");

                #[cfg(feature = "network_handler")]
                {
                    // Try to sync nonce from blockchain
                    let network_handler_lock = wallet.get_network_handler().lock().await;
                    if let Some(network_handler) = network_handler_lock.as_ref() {
                        let address = wallet.get_address();
                        match network_handler.get_api().get_nonce(&address).await {
                            Ok(nonce_result) => {
                                let blockchain_nonce = nonce_result.version.get_nonce();
                                if log::log_enabled!(log::Level::Info) {
                                    info!("Blockchain nonce: {}, topoheight: {}", blockchain_nonce, nonce_result.topoheight);
                                }

                                // Update local nonce to blockchain nonce
                                let mut storage = wallet.get_storage().write().await;
                                if let Err(nonce_err) = storage.set_nonce(blockchain_nonce) {
                                    manager.error(format!("Failed to update nonce: {:#}", nonce_err));
                                } else {
                                    manager.message(format!("Nonce synced from blockchain: {}", blockchain_nonce));

                                    // Clear cache and unconfirmed balances to reflect correct state
                                    storage.clear_tx_cache();
                                    storage.delete_unconfirmed_balances().await;

                                    manager.error("Please retry the transaction with the updated nonce");
                                    return;
                                }
                            }
                            Err(nonce_err) => {
                                if log::log_enabled!(log::Level::Warn) {
                                    warn!("Failed to query nonce from blockchain: {:#}", nonce_err);
                                }
                                manager.error(format!("Failed to sync nonce from blockchain: {:#}", nonce_err));
                            }
                        }
                    }
                }
            }

            manager.error(format!("Couldn't submit transaction: {}", error_msg));
            manager.error("You can try to rescan your balance with the command 'rescan'");

            // Maybe cache is corrupted, clear it
            let mut storage = wallet.get_storage().write().await;
            storage.clear_tx_cache();
            storage.delete_unconfirmed_balances().await;
        } else {
            manager.message("Transaction submitted successfully!");
        }
    } else {
        manager.warn("You are currently offline, transaction cannot be send automatically. Please send it manually to the network.");
        manager.message(format!("Transaction in hex format: {}", tx.to_hex()));
    }
}

/// Freeze TOS to get energy with duration-based rewards
async fn freeze_tos(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("freeze_tos", &args)?;

    let prompt = manager.get_prompt();
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Get amount, duration, and confirm from arguments
    let amount_str = if args.has_argument("amount") {
        args.get_value("amount")?.to_string_value()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("amount".to_string()));
    } else {
        prompt.read_input(
            prompt.colorize_string(Color::Green, "Amount (TOS): "),
            false
        ).await.context("Error while reading amount")?
    };

    let duration_num = if args.has_argument("duration") {
        args.get_value("duration")?.to_number()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("duration".to_string()));
    } else {
        prompt.read(
            prompt.colorize_string(Color::Green, "Duration (3/7/14 days): ")
        ).await.context("Error while reading duration")?
    };

    let confirm_str = if args.has_argument("confirm") {
        args.get_value("confirm")?.to_string_value()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("confirm".to_string()));
    } else {
        prompt.read_input(
            prompt.colorize_string(Color::Green, "Confirm (yes/no): "),
            false
        ).await.context("Error while reading confirmation")?
    };

    // Parse amount
    let amount = from_coin(&amount_str, 8).context("Invalid amount")?;

    // Parse duration (3-90 days)
    let duration = if duration_num >= 3 && duration_num <= 90 {
        tos_common::account::FreezeDuration::new(duration_num as u32)
            .map_err(|e| CommandError::InvalidArgument(e.to_string()))?
    } else {
        return Err(CommandError::InvalidArgument("Duration must be between 3 and 90 days".to_string()));
    };

    // Parse confirmation
    let confirmed = match confirm_str.to_lowercase().as_str() {
        "yes" | "y" | "true" => true,
        "no" | "n" | "false" => false,
        _ => {
            let message = format!(
                "Freeze {} TOS for {:?} to get energy?\nReward multiplier: {}x\n(Y/N): ",
                format_coin(amount, 8),
                duration,
                duration.reward_multiplier()
            );
            prompt.read_valid_str_value(
                prompt.colorize_string(Color::Yellow, &message),
                vec!["y", "n"]
            ).await.context("Error while reading confirmation")? == "y"
        }
    };

    if !confirmed {
        manager.message("Freeze operation cancelled");
        return Ok(());
    }

    // Create freeze transaction
    let duration_clone = duration.clone();
    let _payload = tos_common::transaction::EnergyPayload::FreezeTos {
        amount,
        duration,
    };

    manager.message("Building transaction...");
    
    // Create energy transaction builder with validated parameters
    let energy_builder = EnergyBuilder::freeze_tos(amount, duration_clone.clone());
    
    // Validate the builder configuration before creating transaction
    if let Err(e) = energy_builder.validate() {
        manager.error(&format!("Invalid energy builder configuration: {}", e));
        return Ok(())
    }
    
    let tx_type = TransactionTypeBuilder::Energy(energy_builder);
    let fee = FeeBuilder::default();

    let tx = match wallet.create_transaction(tx_type, fee).await {
        Ok(tx) => tx,
        Err(e) => {
            manager.error(&format!("Error while creating transaction: {}", e));
            return Ok(())
        }
    };

    let hash = tx.hash();
    manager.message(format!("Freeze transaction created: {}", hash));
    manager.message(format!("Amount: {} TOS", format_coin(amount, 8)));
    manager.message(format!("Duration: {:?}", duration_clone));
    manager.message(format!("Reward multiplier: {}x", duration_clone.reward_multiplier()));

    // Update energy resource in storage
    let mut storage = wallet.get_storage().write().await;
    let current_topoheight = if wallet.is_online().await {
        if let Some(network_handler) = wallet.get_network_handler().lock().await.as_ref() {
            match network_handler.get_api().get_info().await {
                Ok(info) => info.topoheight,
                Err(_) => 0,
            }
        } else {
            0
        }
    } else {
        0
    };

    // Get or create energy resource
    let mut energy_resource = if let Some(resource) = storage.get_energy_resource().await? {
        resource.clone()
    } else {
        tos_common::account::EnergyResource::new()
    };

    // Add energy from this freeze operation
    let energy_gained = energy_resource.freeze_tos_for_energy(amount, duration_clone, current_topoheight);
    storage.set_energy_resource(energy_resource).await?;

    manager.message(format!("Energy gained: {} energy", format_coin(energy_gained, 8)));

    // Broadcast the transaction
    broadcast_tx(&wallet, manager, tx).await;

    Ok(())
}

/// Unfreeze TOS (release frozen TOS after lock period)
async fn unfreeze_tos(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    manager.validate_batch_params("unfreeze_tos", &args)?;

    let prompt = manager.get_prompt();
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;

    // Get amount and confirm from arguments
    let amount_str = if args.has_argument("amount") {
        args.get_value("amount")?.to_string_value()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("amount".to_string()));
    } else {
        prompt.read_input(
            prompt.colorize_string(Color::Green, "Amount (TOS): "),
            false
        ).await.context("Error while reading amount")?
    };

    let confirm_str = if args.has_argument("confirm") {
        args.get_value("confirm")?.to_string_value()?
    } else if manager.is_batch_mode() {
        return Err(CommandError::MissingArgument("confirm".to_string()));
    } else {
        prompt.read_input(
            prompt.colorize_string(Color::Green, "Confirm (yes/no): "),
            false
        ).await.context("Error while reading confirmation")?
    };

    let amount = from_coin(&amount_str, 8).context("Invalid amount")?;

    // Parse confirmation
    let confirmed = match confirm_str.to_lowercase().as_str() {
        "yes" | "y" | "true" => true,
        "no" | "n" | "false" => false,
        _ => {
            let message = format!(
                "Unfreeze {} TOS?\nThis will remove the corresponding energy.\n(Y/N): ",
                format_coin(amount, 8)
            );
            prompt.read_valid_str_value(
                prompt.colorize_string(Color::Yellow, &message),
                vec!["y", "n"]
            ).await.context("Error while reading confirmation")? == "y"
        }
    };

    if !confirmed {
        manager.message("Unfreeze operation cancelled");
        return Ok(());
    }

    // Create unfreeze transaction
    let _payload = tos_common::transaction::EnergyPayload::UnfreezeTos {
        amount,
    };

    manager.message("Building transaction...");
    
    // Create energy transaction builder with validated parameters
    let energy_builder = EnergyBuilder::unfreeze_tos(amount);
    
    // Validate the builder configuration before creating transaction
    if let Err(e) = energy_builder.validate() {
        manager.error(&format!("Invalid energy builder configuration: {}", e));
        return Ok(())
    }
    
    let tx_type = TransactionTypeBuilder::Energy(energy_builder);
    let fee = FeeBuilder::default();

    manager.message("Building transaction...");
    let tx = match wallet.create_transaction(tx_type, fee).await {
        Ok(tx) => tx,
        Err(e) => {
            manager.error(&format!("Error while creating transaction: {}", e));
            return Ok(())
        }
    };

    let hash = tx.hash();
    manager.message(format!("Unfreeze transaction created: {}", hash));
    manager.message(format!("Amount: {} TOS", format_coin(amount, 8)));

    // Update energy resource in storage
    let mut storage = wallet.get_storage().write().await;
    let current_topoheight = if wallet.is_online().await {
        if let Some(network_handler) = wallet.get_network_handler().lock().await.as_ref() {
            match network_handler.get_api().get_info().await {
                Ok(info) => info.topoheight,
                Err(_) => 0,
            }
        } else {
            0
        }
    } else {
        0
    };

    // Update energy resource if it exists
    if let Some(mut energy_resource) = storage.get_energy_resource().await?.cloned() {
        match energy_resource.unfreeze_tos(amount, current_topoheight) {
            Ok(energy_removed) => {
                storage.set_energy_resource(energy_resource).await?;
                manager.message(format!("Energy removed: {} energy", format_coin(energy_removed, 8)));
            }
            Err(e) => {
                manager.warn(&format!("Could not update energy resource: {}", e));
                manager.message("Energy resource will be updated when transaction is confirmed");
            }
        }
    }

    // Broadcast the transaction
    broadcast_tx(&wallet, manager, tx).await;

    Ok(())
}

/// Show energy information and freeze records
async fn energy_info(manager: &CommandManager, _args: ArgumentManager) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let wallet: &Arc<Wallet> = context.get()?;
    
    if !wallet.is_online().await {
        manager.error("Wallet is not connected to a daemon. Please enable online mode first.");
        return Ok(());
    }

    let network_handler = wallet.get_network_handler().lock().await;
    if let Some(handler) = network_handler.as_ref() {
        let api = handler.get_api();
        let address = wallet.get_address();
        
        match api.call(&"get_energy".to_string(), &tos_common::api::daemon::GetEnergyParams {
            address: Cow::Borrowed(&address)
        }).await {
            Ok(result) => {
                let energy_result: tos_common::api::daemon::GetEnergyResult = serde_json::from_value(result)
                    .context("Failed to parse energy result")?;
                
                manager.message(format!("Energy Information for {}:", address));
                manager.message(format!("  Frozen TOS: {} TOS", format_tos(energy_result.frozen_tos)));
                manager.message(format!("  Total Energy: {} units", energy_result.total_energy));
                manager.message(format!("  Used Energy: {} units", energy_result.used_energy));
                manager.message(format!("  Available Energy: {} units", energy_result.available_energy));
                manager.message(format!("  Last Update: topoheight {}", energy_result.last_update));
                
                if !energy_result.freeze_records.is_empty() {
                    manager.message("  Freeze Records:");
                    for (i, record) in energy_result.freeze_records.iter().enumerate() {
                        manager.message(format!("    Record {}: {} TOS for {} days", i + 1, format_tos(record.amount), record.duration));
                        manager.message(format!("      Energy Gained: {} units", record.energy_gained));
                        manager.message(format!("      Freeze Time: topoheight {}", record.freeze_topoheight));
                        manager.message(format!("      Unlock Time: topoheight {}", record.unlock_topoheight));
                        
                        if record.can_unlock {
                            manager.message(format!("      Status: ✅ Unlockable"));
                        } else {
                            let remaining_days = record.remaining_blocks as f64 / (24.0 * 60.0 * 60.0);
                            manager.message(format!("      Status: 🔒 Locked ({} days remaining)", remaining_days));
                        }
                    }
                }
            },
            Err(e) => {
                manager.error(format!("Failed to get energy information: {}", e));
            }
        }
    } else {
        manager.error("Wallet is not connected to a daemon");
    }

    Ok(())
}

/// Execute JSON batch command
async fn execute_json_batch(command_manager: &CommandManager, json_content: &str, config: &Config) -> Result<(), anyhow::Error> {
    // Parse JSON
    let json_config: JsonBatchConfig = serde_json::from_str(json_content)
        .with_context(|| "Failed to parse JSON batch configuration")?;

    if log::log_enabled!(log::Level::Info) {
        info!("Executing JSON batch command: {}", json_config.command);
    }

    // Override wallet_path and password from JSON if provided
    // but CLI parameters take precedence
    let _wallet_path = config.wallet_path.as_ref()
        .or(json_config.wallet_path.as_ref())
        .ok_or_else(|| anyhow::anyhow!("No wallet path specified. Use --wallet-path or provide wallet_path in JSON"))?;

    let _password = config.password.as_ref()
        .or(json_config.password.as_ref())
        .ok_or_else(|| anyhow::anyhow!("No password specified. Use --password or provide password in JSON"))?;

    // Store wallet info in command manager context if needed
    // This would require additional setup for wallet loading in JSON mode
    // For now, we assume the wallet is already loaded

    match command_manager.handle_json_command(&json_config.command, &json_config.params).await {
        Ok(_) => {
            info!("JSON batch command executed successfully");
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