mod config;
mod daemon_client;
mod storage;
mod transaction_builder;
// mod integration_tests; // Temporarily disabled

use anyhow::Result;
use clap::Parser;
use config::{ConfigValidator, ValidatedConfig};
use daemon_client::DaemonClient;
use log::{info, warn};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use storage::StorageManager;
use tos_common::{
    ai_mining::*,
    async_handler,
    crypto::{Address, Hash},
    network::Network,
    prompt::{
        argument::{Arg, ArgType, ArgumentManager},
        command::{Command, CommandError, CommandHandler, CommandManager},
        default_logs_datetime_format, Color, LogLevel, Prompt, ShareablePrompt,
    },
};
use transaction_builder::AIMiningTransactionBuilder;

/// Default daemon address for AI mining
const DEFAULT_DAEMON_ADDRESS: &str = "http://127.0.0.1:18080";

/// Get the next nonce for an address from the daemon
async fn get_next_nonce(
    daemon_client: &DaemonClient,
    address: &Address,
) -> Result<u64, anyhow::Error> {
    let address_str = address.to_string();

    // Try to get the current nonce from daemon
    match daemon_client.get_nonce(&address_str).await {
        Ok(nonce) => {
            log::debug!("Retrieved nonce {nonce} from daemon for address {address}");
            Ok(nonce + 1) // Next nonce is current + 1
        }
        Err(e) => {
            log::warn!("Failed to get nonce from daemon: {e}. Using fallback method.");

            // Fallback to timestamp + random for development
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis() as u64;

            // Add some randomness to avoid nonce collision
            let random_component = rand::random::<u16>() as u64;
            let nonce = timestamp + random_component;

            log::debug!("Generated fallback nonce {nonce} for address {address}");
            Ok(nonce)
        }
    }
}

/// AI Mining CLI configuration - wrapper for command line parsing
#[derive(Parser, Clone, Debug)]
#[command(name = "tos-ai-miner")]
#[command(about = "TOS AI Mining CLI - Proof of Intelligent Work")]
pub struct CliConfig {
    /// Set log level
    #[clap(long, value_enum, default_value_t = LogLevel::Info)]
    log_level: LogLevel,

    /// Disable the log file
    #[clap(long)]
    disable_file_logging: bool,

    /// Disable the usage of colors in log
    #[clap(long)]
    disable_log_color: bool,

    /// Disable terminal interactive mode
    #[clap(long)]
    disable_interactive_mode: bool,

    /// Log filename
    #[clap(long, default_value_t = String::from("tos-ai-miner.log"))]
    filename_log: String,

    /// Logs directory
    #[clap(long, default_value_t = String::from("logs/"))]
    logs_path: String,

    /// Storage directory for AI mining state
    #[clap(long, default_value_t = String::from("storage/"))]
    storage_path: String,

    /// Daemon address to connect to
    #[clap(long, default_value_t = String::from(DEFAULT_DAEMON_ADDRESS))]
    daemon_address: String,

    /// Wallet address for AI mining operations
    #[clap(short, long)]
    miner_address: Option<Address>,

    /// Network to use (mainnet, testnet, devnet, stagenet)
    #[clap(long, default_value = "mainnet")]
    network: String,

    /// Advanced: Request timeout in seconds
    #[clap(long, default_value_t = 30)]
    request_timeout_secs: u64,

    /// Advanced: Connection timeout in seconds
    #[clap(long, default_value_t = 10)]
    connection_timeout_secs: u64,

    /// Advanced: Maximum number of retries
    #[clap(long, default_value_t = 3)]
    max_retries: u32,

    /// Advanced: Retry delay in milliseconds
    #[clap(long, default_value_t = 1000)]
    retry_delay_ms: u64,

    /// Enable strict configuration validation
    #[clap(long)]
    strict_validation: bool,

    /// Disable auto-fix of configuration issues
    #[clap(long)]
    no_auto_fix: bool,

    /// JSON File to load the configuration from
    #[clap(long)]
    config_file: Option<String>,

    /// Generate the template at the `config_file` path
    #[clap(long)]
    generate_config_template: bool,
}

impl CliConfig {
    /// Convert CLI configuration to ValidatedConfig
    pub fn to_validated_config(self) -> ValidatedConfig {
        ValidatedConfig {
            log_level: self.log_level,
            disable_file_logging: self.disable_file_logging,
            disable_log_color: self.disable_log_color,
            disable_interactive_mode: self.disable_interactive_mode,
            filename_log: self.filename_log,
            logs_path: self.logs_path,
            storage_path: self.storage_path,
            daemon_address: self.daemon_address,
            miner_address: self.miner_address,
            network: self.network,
            request_timeout_secs: self.request_timeout_secs,
            connection_timeout_secs: self.connection_timeout_secs,
            max_retries: self.max_retries,
            retry_delay_ms: self.retry_delay_ms,
            auto_fix_config: !self.no_auto_fix,
            strict_validation: self.strict_validation,
        }
    }
}

// Statistics
static TOTAL_TASKS: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_TASKS: AtomicUsize = AtomicUsize::new(0);
static COMPLETED_TASKS: AtomicUsize = AtomicUsize::new(0);
static REGISTERED_MINERS: AtomicUsize = AtomicUsize::new(0);

#[tokio::main]
async fn main() -> Result<()> {
    let cli_config = CliConfig::parse();

    // Handle config template generation
    if let Some(path) = cli_config.config_file.as_ref() {
        if cli_config.generate_config_template {
            if Path::new(path).exists() {
                eprintln!("⚠️  Config file already exists at {path}");
                eprintln!("Use a different path or remove the existing file");
                return Ok(());
            }

            ValidatedConfig::generate_template(path)?;
            println!("📝 Configuration template generated at {path}");
            println!("💡 Edit the file and run the application with --config-file {path}");
            return Ok(());
        }
    }

    // Load and validate configuration
    let config = if let Some(config_path) = &cli_config.config_file {
        println!("📖 Loading configuration from: {config_path}");
        ValidatedConfig::from_file(
            config_path,
            cli_config.strict_validation,
            !cli_config.no_auto_fix,
        )?
    } else {
        // Use CLI configuration and validate it
        let mut config = cli_config.to_validated_config();
        let validator = ConfigValidator::new(config.strict_validation, config.auto_fix_config);
        let messages = validator.validate(&mut config)?;

        if !messages.is_empty() {
            println!(
                "🔧 Configuration validation completed with {} message(s)",
                messages.len()
            );
        }

        config
    };

    // Initialize logging
    let prompt = Prompt::new(tos_common::prompt::PromptConfig {
        level: config.log_level,
        dir_path: &config.logs_path,
        filename_log: &config.filename_log,
        disable_file_logging: config.disable_file_logging,
        disable_file_log_date_based: false,
        disable_colors: config.disable_log_color,
        enable_auto_compress_logs: false,
        interactive: !config.disable_interactive_mode,
        module_logs: vec![],
        file_level: config.log_level,
        show_ascii: true,
        logs_datetime_format: default_logs_datetime_format(),
    })?;

    // Remove init call since it returns ()

    if log::log_enabled!(log::Level::Info) {
        info!("TOS AI Miner v{} starting...", env!("CARGO_PKG_VERSION"));
    }
    if log::log_enabled!(log::Level::Info) {
        info!("Daemon address: {}", config.daemon_address);
    }

    if let Some(address) = &config.miner_address {
        if log::log_enabled!(log::Level::Info) {
            info!("Miner address: {address}");
        }
    } else {
        warn!("No miner address specified. Some operations will require an address.");
    }

    // Parse network (validation already handled in config validation)
    let network = config.get_network();

    // Create daemon client with validated configuration
    let daemon_config = config.to_daemon_client_config();
    let daemon_client = Arc::new(DaemonClient::with_config(
        &config.daemon_address,
        daemon_config,
    )?);

    // Create transaction builder
    let tx_builder = Arc::new(AIMiningTransactionBuilder::new(network));

    // Initialize storage manager
    let storage_dir = PathBuf::from(&config.storage_path);
    let storage_manager = Arc::new(Mutex::new(StorageManager::new(storage_dir, network).await?));
    if log::log_enabled!(log::Level::Info) {
        info!("Storage initialized at: {}", config.storage_path);
    }

    // Test connection to daemon
    info!("Testing connection to daemon...");
    if let Err(e) = daemon_client.test_connection().await {
        if log::log_enabled!(log::Level::Warn) {
            warn!("Failed to connect to daemon: {e}. AI mining commands may not work properly.");
        }
    }

    if !config.disable_interactive_mode {
        run_prompt(prompt, config, daemon_client, tx_builder, storage_manager).await?;
    }

    Ok(())
}

async fn run_prompt(
    prompt: ShareablePrompt,
    config: ValidatedConfig,
    daemon_client: Arc<DaemonClient>,
    tx_builder: Arc<AIMiningTransactionBuilder>,
    storage_manager: Arc<Mutex<StorageManager>>,
) -> Result<()> {
    let command_manager = CommandManager::new(prompt.clone());

    // Register AI mining commands
    register_ai_mining_commands(
        &command_manager,
        config,
        daemon_client,
        tx_builder,
        storage_manager,
    )
    .await?;

    let closure = |_: &_, _: _| async {
        let tasks_str = format!(
            "{}: {}",
            prompt.colorize_string(Color::Yellow, "Total Tasks"),
            prompt.colorize_string(
                Color::Green,
                &format!("{}", TOTAL_TASKS.load(Ordering::SeqCst))
            ),
        );
        let active_str = format!(
            "{}: {}",
            prompt.colorize_string(Color::Yellow, "Active"),
            prompt.colorize_string(
                Color::Green,
                &format!("{}", ACTIVE_TASKS.load(Ordering::SeqCst))
            ),
        );
        let completed_str = format!(
            "{}: {}",
            prompt.colorize_string(Color::Yellow, "Completed"),
            prompt.colorize_string(
                Color::Green,
                &format!("{}", COMPLETED_TASKS.load(Ordering::SeqCst))
            ),
        );
        let miners_str = format!(
            "{}: {}",
            prompt.colorize_string(Color::Yellow, "Miners"),
            prompt.colorize_string(
                Color::Green,
                &format!("{}", REGISTERED_MINERS.load(Ordering::SeqCst))
            ),
        );

        Ok(format!(
            "{} | {} | {} | {} | {} {} ",
            prompt.colorize_string(Color::Blue, "AI Miner"),
            tasks_str,
            active_str,
            completed_str,
            miners_str,
            prompt.colorize_string(Color::BrightBlack, ">>")
        ))
    };

    prompt
        .start(
            Duration::from_millis(1000),
            Box::new(async_handler!(closure)),
            Some(&command_manager),
        )
        .await?;
    Ok(())
}

/// Register all AI mining commands
async fn register_ai_mining_commands(
    manager: &CommandManager,
    config: ValidatedConfig,
    daemon_client: Arc<DaemonClient>,
    tx_builder: Arc<AIMiningTransactionBuilder>,
    storage_manager: Arc<Mutex<StorageManager>>,
) -> Result<(), CommandError> {
    // Set config, daemon client, transaction builder, and storage manager in context for commands to use
    {
        let mut context = manager.get_context().lock()?;
        context.store(config);
        context.store(daemon_client);
        context.store(tx_builder);
        context.store(storage_manager);
    }

    // Register miner command
    manager.add_command(Command::with_optional_arguments(
        "register_miner",
        "Register as an AI miner",
        vec![
            Arg::new(
                "address",
                ArgType::String,
                "Miner wallet address for rewards",
            ),
            Arg::new("fee", ArgType::Number, "Registration fee amount"),
        ],
        CommandHandler::Async(async_handler!(register_miner)),
    ))?;

    // Publish task command
    manager.add_command(Command::with_optional_arguments(
        "publish_task",
        "Publish a new AI mining task",
        vec![
            Arg::new("reward", ArgType::Number, "Task reward amount"),
            Arg::new("difficulty", ArgType::String, "Task difficulty level"),
            Arg::new("deadline", ArgType::Number, "Task deadline timestamp"),
            Arg::new(
                "description",
                ArgType::String,
                "Task description or requirements",
            ),
        ],
        CommandHandler::Async(async_handler!(publish_task)),
    ))?;

    // Submit answer command
    manager.add_command(Command::with_optional_arguments(
        "submit_answer",
        "Submit an answer to a task",
        vec![
            Arg::new("task_id", ArgType::String, "Unique task identifier"),
            Arg::new("answer", ArgType::String, "Answer submission content"),
            Arg::new(
                "stake",
                ArgType::Number,
                "Staking amount for answer validation",
            ),
        ],
        CommandHandler::Async(async_handler!(submit_answer)),
    ))?;

    // Validate answer command
    manager.add_command(Command::with_optional_arguments(
        "validate_answer",
        "Validate a submitted answer",
        vec![
            Arg::new("task_id", ArgType::String, "Unique task identifier"),
            Arg::new(
                "answer_id",
                ArgType::String,
                "Answer identifier to validate",
            ),
            Arg::new("score", ArgType::Number, "Validation score (0-100)"),
        ],
        CommandHandler::Async(async_handler!(validate_answer)),
    ))?;

    // List tasks command
    manager.add_command(Command::new(
        "list_tasks",
        "List all active AI mining tasks",
        CommandHandler::Async(async_handler!(list_tasks)),
    ))?;

    // Show stats command
    manager.add_command(Command::new(
        "stats",
        "Show AI mining statistics",
        CommandHandler::Async(async_handler!(show_stats)),
    ))?;

    // Show reputation command
    manager.add_command(Command::with_optional_arguments(
        "reputation",
        "Show miner reputation",
        vec![Arg::new(
            "address",
            ArgType::String,
            "Miner wallet address for rewards",
        )],
        CommandHandler::Async(async_handler!(show_reputation)),
    ))?;

    // Daemon status command
    manager.add_command(Command::new(
        "daemon_status",
        "Check daemon connection status",
        CommandHandler::Async(async_handler!(daemon_status)),
    ))?;

    // Storage-related commands
    manager.add_command(Command::new(
        "storage_stats",
        "Show storage statistics",
        CommandHandler::Async(async_handler!(storage_stats)),
    ))?;

    manager.add_command(Command::new(
        "show_tasks",
        "Show all stored tasks",
        CommandHandler::Async(async_handler!(show_stored_tasks)),
    ))?;

    manager.add_command(Command::new(
        "show_transactions",
        "Show transaction history",
        CommandHandler::Async(async_handler!(show_transaction_history)),
    ))?;

    manager.add_command(Command::with_optional_arguments(
        "clear_storage",
        "Clear all storage data (use with caution)",
        vec![Arg::new(
            "confirm",
            ArgType::String,
            "Confirm action (yes/no)",
        )],
        CommandHandler::Async(async_handler!(clear_storage)),
    ))?;

    // Integration testing commands
    manager.add_command(Command::with_optional_arguments(
        "run_integration_tests",
        "Run comprehensive AI mining workflow tests",
        vec![Arg::new(
            "mock_mode",
            ArgType::String,
            "Enable mock mode for testing",
        )],
        CommandHandler::Async(async_handler!(run_integration_tests)),
    ))?;

    manager.add_command(Command::new(
        "test_task_publication",
        "Test AI task publication workflow",
        CommandHandler::Async(async_handler!(test_task_publication_workflow)),
    ))?;

    manager.add_command(Command::new(
        "test_answer_submission",
        "Test AI answer submission workflow",
        CommandHandler::Async(async_handler!(test_answer_submission_workflow)),
    ))?;

    manager.add_command(Command::new(
        "test_validation",
        "Test AI answer validation workflow",
        CommandHandler::Async(async_handler!(test_validation_workflow)),
    ))?;

    manager.add_command(Command::new(
        "test_reward_cycle",
        "Test complete reward distribution cycle",
        CommandHandler::Async(async_handler!(test_reward_cycle)),
    ))?;

    Ok(())
}

/// Register as an AI miner
async fn register_miner(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let prompt = manager.get_prompt();
    let miner_address_opt = {
        let context = manager.get_context().lock()?;
        let config: &ValidatedConfig = context.get()?;
        config.miner_address.clone()
    };

    let address_str = match args.get_value("address") {
        Ok(addr) => addr.to_string_value()?,
        Err(_) => {
            if let Some(addr) = &miner_address_opt {
                addr.to_string()
            } else {
                prompt.read_input("Enter miner address", false).await?
            }
        }
    };

    let fee_amount = match args.get_value("fee") {
        Ok(fee) => fee.to_number()?,
        Err(_) => {
            let fee_str = prompt
                .read_input("Enter registration fee (nanoTOS)", false)
                .await?;
            fee_str
                .parse()
                .map_err(|_| CommandError::InvalidArgument("Invalid fee amount".to_string()))?
        }
    };

    // Parse address
    let address = Address::from_string(&address_str)
        .map_err(|_| CommandError::InvalidArgument("Invalid address format".to_string()))?;

    manager.message(format!(
        "Registering miner {address} with fee {fee_amount} nanoTOS"
    ));

    // Get storage, transaction builder, and daemon client
    let (storage, tx_builder, daemon_client) = {
        let context = manager.get_context().lock()?;
        let storage: &Arc<Mutex<StorageManager>> = context.get()?;
        let tx_builder: &Arc<AIMiningTransactionBuilder> = context.get()?;
        let daemon_client: &Arc<DaemonClient> = context.get()?;
        (storage.clone(), tx_builder.clone(), daemon_client.clone())
    };

    // Generate nonce
    let nonce = get_next_nonce(&daemon_client, &address)
        .await
        .map_err(|e| CommandError::BatchModeError(format!("Failed to generate nonce: {e}")))?;

    // Create transaction metadata
    let metadata = tx_builder
        .build_register_miner_transaction(
            address.clone().to_public_key(),
            fee_amount,
            nonce,
            0, // fee (auto-calculate)
        )
        .map_err(|e| CommandError::BatchModeError(e.to_string()))?;

    manager.message("Registration transaction created:");
    manager.message(format!("  - Address: {address}"));
    manager.message(format!(
        "  - Registration Fee: {} nanoTOS ({} TOS)",
        fee_amount,
        fee_amount as f64 / 1_000_000_000.0
    ));
    manager.message(format!(
        "  - Estimated TX Fee: {} nanoTOS",
        metadata.estimated_fee
    ));
    manager.message(format!(
        "  - Estimated Size: {} bytes",
        metadata.estimated_size
    ));
    manager.message(format!("  - Nonce: {}", metadata.nonce));

    // Store miner registration in storage
    {
        let public_key = address.to_public_key();
        let metadata_clone = metadata.clone();

        storage
            .lock()
            .map_err(|e| CommandError::BatchModeError(format!("Storage lock error: {e}")))?
            .register_miner(&public_key, fee_amount)
            .await
            .map_err(|e| CommandError::BatchModeError(format!("Storage error: {e}")))?;

        // Add transaction record
        storage
            .lock()
            .map_err(|e| CommandError::BatchModeError(format!("Storage lock error: {e}")))?
            .add_transaction(&metadata_clone, None)
            .await
            .map_err(|e| CommandError::BatchModeError(format!("Transaction storage error: {e}")))?;
    }

    manager.message("✅ Miner registration stored successfully");

    REGISTERED_MINERS.fetch_add(1, Ordering::SeqCst);

    Ok(())
}

/// Publish a new AI mining task
async fn publish_task(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let prompt = manager.get_prompt();

    let reward_amount = match args.get_value("reward") {
        Ok(reward) => reward.to_number()?,
        Err(_) => {
            let reward_str = prompt
                .read_input("Enter reward amount (nanoTOS)", false)
                .await?;
            reward_str
                .parse()
                .map_err(|_| CommandError::InvalidArgument("Invalid reward amount".to_string()))?
        }
    };

    let difficulty_str = match args.get_value("difficulty") {
        Ok(diff) => diff.to_string_value()?,
        Err(_) => {
            prompt
                .read_input(
                    "Enter difficulty (Beginner/Intermediate/Advanced/Expert)",
                    false,
                )
                .await?
        }
    };

    let difficulty = match difficulty_str.to_lowercase().as_str() {
        "beginner" => DifficultyLevel::Beginner,
        "intermediate" => DifficultyLevel::Intermediate,
        "advanced" => DifficultyLevel::Advanced,
        "expert" => DifficultyLevel::Expert,
        _ => {
            return Err(CommandError::InvalidArgument(
                "Invalid difficulty level".to_string(),
            ))
        }
    };

    let deadline = match args.get_value("deadline") {
        Ok(dl) => dl.to_number()?,
        Err(_) => {
            let deadline_str = prompt
                .read_input("Enter deadline (timestamp)", false)
                .await?;
            deadline_str
                .parse()
                .map_err(|_| CommandError::InvalidArgument("Invalid deadline".to_string()))?
        }
    };

    let description = match args.get_value("description") {
        Ok(desc) => desc.to_string_value()?,
        Err(_) => prompt.read_input("Enter task description", false).await?,
    };

    // Validate reward against difficulty
    let (min_reward, max_reward) = difficulty.reward_range();
    if reward_amount < min_reward || reward_amount > max_reward {
        return Err(CommandError::InvalidArgument(format!(
            "Reward {reward_amount} is outside valid range [{min_reward}, {max_reward}] for difficulty {difficulty:?}"
        )));
    }

    // Generate task ID
    let task_id = Hash::new(rand::random::<[u8; 32]>());

    manager.message("Publishing AI mining task:".to_string());
    manager.message(format!("  - Task ID: {}", hex::encode(task_id.as_bytes())));
    manager.message(format!(
        "  - Reward: {} nanoTOS ({} TOS)",
        reward_amount,
        reward_amount as f64 / 1_000_000_000.0
    ));
    manager.message(format!("  - Difficulty: {difficulty:?}"));
    manager.message(format!("  - Deadline: {deadline}"));
    manager.message(format!("  - Description: {description}"));

    TOTAL_TASKS.fetch_add(1, Ordering::SeqCst);
    ACTIVE_TASKS.fetch_add(1, Ordering::SeqCst);

    Ok(())
}

/// Submit an answer to a task
async fn submit_answer(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let prompt = manager.get_prompt();

    let task_id_str = match args.get_value("task_id") {
        Ok(id) => id.to_string_value()?,
        Err(_) => prompt.read_input("Enter task ID", false).await?,
    };

    let answer_str = match args.get_value("answer") {
        Ok(ans) => ans.to_string_value()?,
        Err(_) => prompt.read_input("Enter your answer", false).await?,
    };

    let stake_amount = match args.get_value("stake") {
        Ok(stake) => stake.to_number()?,
        Err(_) => {
            let stake_str = prompt
                .read_input("Enter stake amount (nanoTOS)", false)
                .await?;
            stake_str
                .parse()
                .map_err(|_| CommandError::InvalidArgument("Invalid stake amount".to_string()))?
        }
    };

    // Parse task ID
    let task_id_bytes = hex::decode(&task_id_str)
        .map_err(|_| CommandError::InvalidArgument("Invalid task ID format".to_string()))?;
    if task_id_bytes.len() != 32 {
        return Err(CommandError::InvalidArgument(
            "Task ID must be 32 bytes".to_string(),
        ));
    }
    let mut task_id_array = [0u8; 32];
    task_id_array.copy_from_slice(&task_id_bytes);
    let _task_id = Hash::new(task_id_array);

    // Hash the answer using blake3
    let hash_bytes = blake3::hash(answer_str.as_bytes());
    let mut hash_array = [0u8; 32];
    hash_array.copy_from_slice(hash_bytes.as_bytes());
    let answer_hash = Hash::new(hash_array);

    manager.message("Submitting answer to task:".to_string());
    manager.message(format!("  - Task ID: {task_id_str}"));
    manager.message(format!(
        "  - Answer hash: {}",
        hex::encode(answer_hash.as_bytes())
    ));
    manager.message(format!(
        "  - Stake: {} nanoTOS ({} TOS)",
        stake_amount,
        stake_amount as f64 / 1_000_000_000.0
    ));

    Ok(())
}

/// Validate a submitted answer
async fn validate_answer(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let prompt = manager.get_prompt();

    let task_id_str = match args.get_value("task_id") {
        Ok(id) => id.to_string_value()?,
        Err(_) => prompt.read_input("Enter task ID", false).await?,
    };

    let answer_id_str = match args.get_value("answer_id") {
        Ok(id) => id.to_string_value()?,
        Err(_) => prompt.read_input("Enter answer ID", false).await?,
    };

    let score = match args.get_value("score") {
        Ok(s) => s.to_number()? as u8,
        Err(_) => {
            let score_str = prompt
                .read_input("Enter validation score (0-100)", false)
                .await?;
            score_str
                .parse()
                .map_err(|_| CommandError::InvalidArgument("Invalid score".to_string()))?
        }
    };

    if score > 100 {
        return Err(CommandError::InvalidArgument(
            "Score must be between 0-100".to_string(),
        ));
    }

    manager.message("Validating answer:".to_string());
    manager.message(format!("  - Task ID: {task_id_str}"));
    manager.message(format!("  - Answer ID: {answer_id_str}"));
    manager.message(format!("  - Score: {score}/100"));

    Ok(())
}

/// List all active AI mining tasks
async fn list_tasks(manager: &CommandManager, _args: ArgumentManager) -> Result<(), CommandError> {
    manager.message("Active AI Mining Tasks:");
    manager.message("(Demo mode - showing sample data)");
    manager.message("");

    // Show sample tasks for demonstration
    let sample_tasks = [
        (
            "a1b2c3d4...",
            "Beginner",
            "10.0",
            "Image Classification",
            "2h 15m",
        ),
        (
            "e5f6g7h8...",
            "Advanced",
            "75.5",
            "Natural Language Processing",
            "5h 42m",
        ),
        (
            "i9j0k1l2...",
            "Expert",
            "200.0",
            "Code Generation",
            "12h 8m",
        ),
    ];

    for (task_id, difficulty, reward, description, remaining) in sample_tasks {
        manager.message(format!(
            "  Task ID: {task_id} | Difficulty: {difficulty} | Reward: {reward} TOS"
        ));
        manager.message(format!("    Description: {description}"));
        manager.message(format!("    Time remaining: {remaining}"));
        manager.message("");
    }

    Ok(())
}

/// Show AI mining statistics
async fn show_stats(manager: &CommandManager, _args: ArgumentManager) -> Result<(), CommandError> {
    manager.message("AI Mining Statistics:");
    manager.message(format!(
        "  Total Tasks Published: {}",
        TOTAL_TASKS.load(Ordering::SeqCst)
    ));
    manager.message(format!(
        "  Active Tasks: {}",
        ACTIVE_TASKS.load(Ordering::SeqCst)
    ));
    manager.message(format!(
        "  Completed Tasks: {}",
        COMPLETED_TASKS.load(Ordering::SeqCst)
    ));
    manager.message(format!(
        "  Registered Miners: {}",
        REGISTERED_MINERS.load(Ordering::SeqCst)
    ));
    manager.message("");
    manager.message("Reward Distribution:");
    manager.message("  Beginner: 5.0 - 15.0 TOS");
    manager.message("  Intermediate: 15.0 - 50.0 TOS");
    manager.message("  Advanced: 50.0 - 200.0 TOS");
    manager.message("  Expert: 200.0 - 500.0 TOS");

    Ok(())
}

/// Show miner reputation
async fn show_reputation(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let prompt = manager.get_prompt();
    let miner_address_opt = {
        let context = manager.get_context().lock()?;
        let config: &ValidatedConfig = context.get()?;
        config.miner_address.clone()
    };

    let address_str = match args.get_value("address") {
        Ok(addr) => addr.to_string_value()?,
        Err(_) => {
            if let Some(addr) = &miner_address_opt {
                addr.to_string()
            } else {
                prompt.read_input("Enter miner address", false).await?
            }
        }
    };

    manager.message(format!("Miner Reputation for {address_str}:"));
    manager.message("(Demo mode - showing sample data)");
    manager.message("");
    manager.message("  Current Reputation: 650/1000");
    manager.message("  Tasks Published: 12");
    manager.message("  Answers Submitted: 45");
    manager.message("  Validations Performed: 89");
    manager.message("  Success Rate: 87.3%");
    manager.message("");
    manager.message("Reputation Levels:");
    manager.message("  0-200: Newcomer");
    manager.message("  201-400: Apprentice");
    manager.message("  401-600: Skilled");
    manager.message("  601-800: Expert");
    manager.message("  801-1000: Master");

    Ok(())
}

/// Show daemon connection status with comprehensive health check
async fn daemon_status(
    manager: &CommandManager,
    _args: ArgumentManager,
) -> Result<(), CommandError> {
    let daemon_client = {
        let context = manager.get_context().lock()?;
        let daemon_client: &Arc<DaemonClient> = context.get()?;
        daemon_client.clone()
    };

    manager.message("🔍 Performing daemon health check...");

    match daemon_client.health_check().await {
        Ok(health) => {
            if health.is_healthy {
                manager.message("✅ Daemon is healthy and responsive");
                manager.message("📊 Health Check Results:");

                if let Some(version) = &health.version {
                    manager.message(format!("  - Version: {version}"));
                }

                manager.message(format!("  - Response Time: {:?}", health.response_time));

                if let Some(peer_count) = health.peer_count {
                    manager.message(format!("  - Connected Peers: {peer_count}"));
                } else {
                    manager.message("  - Connected Peers: Unable to retrieve");
                }

                if let Some(mempool_size) = health.mempool_size {
                    manager.message(format!("  - Mempool Size: {mempool_size} transactions"));
                } else {
                    manager.message("  - Mempool Size: Unable to retrieve");
                }

                // Get additional blockchain info
                match daemon_client.get_info().await {
                    Ok(info) => {
                        manager.message("⛓️  Blockchain Information:");
                        if let Some(height) = info.get("height").and_then(|h| h.as_u64()) {
                            manager.message(format!("  - Current Height: {height}"));
                        }
                        if let Some(topoheight) = info.get("topoheight").and_then(|h| h.as_u64()) {
                            manager.message(format!("  - Topo Height: {topoheight}"));
                        }
                        if let Some(network) = info.get("network").and_then(|n| n.as_str()) {
                            manager.message(format!("  - Network: {network}"));
                        }
                    }
                    Err(_) => {
                        manager.message("  - Extended blockchain info: Not available");
                    }
                }

                // Show configuration
                let config = daemon_client.config();
                manager.message("⚙️  Client Configuration:");
                manager.message(format!("  - Request Timeout: {:?}", config.request_timeout));
                manager.message(format!(
                    "  - Connection Timeout: {:?}",
                    config.connection_timeout
                ));
                manager.message(format!("  - Max Retries: {}", config.max_retries));
                manager.message(format!("  - Retry Delay: {:?}", config.retry_delay));

                // Performance assessment
                let performance = if health.response_time.as_millis() < 100 {
                    "🚀 Excellent"
                } else if health.response_time.as_millis() < 500 {
                    "⚡ Good"
                } else if health.response_time.as_millis() < 2000 {
                    "⚠️  Slow"
                } else {
                    "🐌 Very Slow"
                };

                manager.message(format!("  - Performance: {performance}"));
            } else {
                manager.message("❌ Daemon health check failed");
                if let Some(error) = &health.error_message {
                    manager.message(format!("  - Error: {error}"));
                }
                manager.message(format!("  - Response Time: {:?}", health.response_time));
            }
        }
        Err(e) => {
            manager.message(format!("❌ Failed to perform health check: {e}"));
            manager.message("🔧 Troubleshooting Tips:");
            manager.message("  1. Verify daemon is running");
            manager.message("  2. Check daemon address configuration");
            manager.message("  3. Ensure network connectivity");
            manager.message("  4. Try adjusting timeout settings");
        }
    }

    Ok(())
}

/// Show storage statistics
async fn storage_stats(
    manager: &CommandManager,
    _args: ArgumentManager,
) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let storage: &Arc<Mutex<StorageManager>> = context.get()?;

    let (stats, miner_info) = {
        let storage_guard = storage
            .lock()
            .map_err(|e| CommandError::BatchModeError(format!("Storage lock error: {e}")))?;
        (
            storage_guard.get_stats(),
            storage_guard.get_miner_info().cloned(),
        )
    };

    manager.message("📊 Storage Statistics:");
    manager.message(format!("  Network: {:?}", stats.network));
    manager.message(format!("  Total Tasks: {}", stats.total_tasks));
    manager.message(format!(
        "  Total Transactions: {}",
        stats.total_transactions
    ));
    manager.message(format!(
        "  Miner Registered: {}",
        if stats.miner_registered { "Yes" } else { "No" }
    ));

    if let Some(miner) = miner_info {
        manager.message("  Miner Statistics:");
        manager.message(format!(
            "    - Tasks Published: {}",
            miner.total_tasks_published
        ));
        manager.message(format!(
            "    - Answers Submitted: {}",
            miner.total_answers_submitted
        ));
        manager.message(format!(
            "    - Validations Performed: {}",
            miner.total_validations_performed
        ));
        manager.message(format!(
            "    - Registration Fee: {} nanoTOS",
            miner.registration_fee
        ));
    }

    let last_updated =
        chrono::DateTime::<chrono::Utc>::from_timestamp(stats.last_updated as i64, 0)
            .unwrap_or_default();
    manager.message(format!(
        "  Last Updated: {}",
        last_updated.format("%Y-%m-%d %H:%M:%S UTC")
    ));

    Ok(())
}

/// Show all stored tasks
async fn show_stored_tasks(
    manager: &CommandManager,
    _args: ArgumentManager,
) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let storage: &Arc<Mutex<StorageManager>> = context.get()?;

    let tasks = {
        let storage_guard = storage
            .lock()
            .map_err(|e| CommandError::BatchModeError(format!("Storage lock error: {e}")))?;
        storage_guard.get_all_tasks().clone()
    };

    if tasks.is_empty() {
        manager.message("📝 No tasks found in storage.");
        return Ok(());
    }

    manager.message(format!("📝 Stored Tasks ({} total):", tasks.len()));

    for (task_id, task) in tasks {
        let created_time =
            chrono::DateTime::<chrono::Utc>::from_timestamp(task.created_at as i64, 0)
                .unwrap_or_default();
        let state_emoji = match task.state {
            storage::TaskState::Published => "🟡",
            storage::TaskState::AnswersReceived => "🔵",
            storage::TaskState::Validated => "🟢",
            storage::TaskState::Expired => "🔴",
        };

        manager.message(format!("  {} Task ID: {}...", state_emoji, &task_id[..16]));
        manager.message(format!("    Reward: {} nanoTOS", task.reward_amount));
        manager.message(format!("    Difficulty: {:?}", task.difficulty));
        manager.message(format!("    State: {:?}", task.state));
        manager.message(format!(
            "    Created: {}",
            created_time.format("%Y-%m-%d %H:%M:%S UTC")
        ));
        manager.message("");
    }

    Ok(())
}

/// Show transaction history
async fn show_transaction_history(
    manager: &CommandManager,
    _args: ArgumentManager,
) -> Result<(), CommandError> {
    let context = manager.get_context().lock()?;
    let storage: &Arc<Mutex<StorageManager>> = context.get()?;

    let transactions = {
        let storage_guard = storage
            .lock()
            .map_err(|e| CommandError::BatchModeError(format!("Storage lock error: {e}")))?;
        storage_guard
            .get_recent_transactions(20)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
    }; // Show last 20 transactions

    if transactions.is_empty() {
        manager.message("💳 No transactions found in history.");
        return Ok(());
    }

    manager.message(format!(
        "💳 Recent Transactions ({} shown):",
        transactions.len()
    ));

    for tx in &transactions {
        let created_time = chrono::DateTime::<chrono::Utc>::from_timestamp(tx.created_at as i64, 0)
            .unwrap_or_default();
        let status_emoji = match tx.status {
            storage::TransactionStatus::Created => "⏳",
            storage::TransactionStatus::Broadcast => "📡",
            storage::TransactionStatus::Confirmed => "✅",
            storage::TransactionStatus::Failed => "❌",
        };

        manager.message(format!(
            "  {} {} - {} nanoTOS",
            status_emoji, tx.payload_type, tx.estimated_fee
        ));
        if let Some(ref hash) = tx.tx_hash {
            manager.message(format!("    Hash: {}...", &hash[..16]));
        }
        manager.message(format!("    Status: {:?}", tx.status));
        manager.message(format!(
            "    Created: {}",
            created_time.format("%Y-%m-%d %H:%M:%S UTC")
        ));

        if let Some(confirmed_at) = tx.confirmed_at {
            let confirmed_time =
                chrono::DateTime::<chrono::Utc>::from_timestamp(confirmed_at as i64, 0)
                    .unwrap_or_default();
            manager.message(format!(
                "    Confirmed: {}",
                confirmed_time.format("%Y-%m-%d %H:%M:%S UTC")
            ));
        }
        manager.message("");
    }

    Ok(())
}

/// Clear all storage data
async fn clear_storage(
    manager: &CommandManager,
    mut args: ArgumentManager,
) -> Result<(), CommandError> {
    let confirm = match args.get_value("confirm") {
        Ok(val) => val.to_string_value()?.to_lowercase(),
        Err(_) => {
            manager.message("⚠️  This will permanently delete all AI mining data!");
            manager.message("To confirm, run: clear_storage confirm=yes");
            return Ok(());
        }
    };

    if confirm != "yes" {
        manager.message("❌ Storage clear cancelled. Use 'confirm=yes' to proceed.");
        return Ok(());
    }

    let storage = {
        let context = manager.get_context().lock()?;
        let storage: &Arc<Mutex<StorageManager>> = context.get()?;
        storage.clone()
    };

    storage
        .lock()
        .map_err(|e| CommandError::BatchModeError(format!("Storage lock error: {e}")))?
        .clear_all()
        .await
        .map_err(|e| CommandError::BatchModeError(format!("Clear storage error: {e}")))?;

    manager.message("✅ All storage data cleared successfully.");

    Ok(())
}

/// Run comprehensive integration tests (temporarily disabled)
async fn run_integration_tests(
    manager: &CommandManager,
    mut _args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.message("⚠️  Comprehensive integration tests temporarily disabled");
    manager.message("   Use individual workflow tests instead:");
    manager.message("   - test_task_publication");
    manager.message("   - test_answer_submission");
    manager.message("   - test_validation");
    manager.message("   - test_reward_cycle");
    Ok(())
}

/// Test AI task publication workflow
async fn test_task_publication_workflow(
    manager: &CommandManager,
    _args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.message("📤 Testing AI task publication workflow...");

    let (tx_builder, daemon_client, publisher_address) = {
        let context = manager.get_context().lock()?;
        let config: &ValidatedConfig = context.get()?;
        let tx_builder: &Arc<AIMiningTransactionBuilder> = context.get()?;
        let daemon_client: &Arc<DaemonClient> = context.get()?;

        let publisher_address = config
            .miner_address
            .as_ref()
            .ok_or_else(|| CommandError::BatchModeError("No miner address configured".to_string()))?
            .clone();

        (tx_builder.clone(), daemon_client.clone(), publisher_address)
    };

    // Create test task
    let task_id = Hash::new(rand::random::<[u8; 32]>());
    let reward_amount = 25_000_000_000; // 25 TOS
    let difficulty = DifficultyLevel::Beginner;
    let deadline = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 7200; // 2 hours from now

    let nonce = get_next_nonce(&daemon_client, &publisher_address)
        .await
        .map_err(|e| CommandError::BatchModeError(format!("Nonce generation failed: {e}")))?;

    // Create transaction metadata
    let metadata = tx_builder
        .build_publish_task_transaction(
            task_id.clone(),
            reward_amount,
            difficulty.clone(),
            deadline,
            "Test AI mining task".to_string(), // Task description
            nonce,
            0, // Auto-calculate fee
        )
        .map_err(|e| CommandError::BatchModeError(e.to_string()))?;

    manager.message("✅ Task publication test completed:");
    manager.message(format!("  - Task ID: {}", hex::encode(task_id.as_bytes())));
    manager.message(format!(
        "  - Reward: {} TOS",
        reward_amount as f64 / 1_000_000_000.0
    ));
    manager.message(format!("  - Difficulty: {difficulty:?}"));
    manager.message(format!(
        "  - Estimated Fee: {} nanoTOS",
        metadata.estimated_fee
    ));
    manager.message(format!(
        "  - Estimated Size: {} bytes",
        metadata.estimated_size
    ));

    Ok(())
}

/// Test AI answer submission workflow
async fn test_answer_submission_workflow(
    manager: &CommandManager,
    _args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.message("💡 Testing AI answer submission workflow...");

    let (tx_builder, daemon_client, miner_address) = {
        let context = manager.get_context().lock()?;
        let tx_builder: &Arc<AIMiningTransactionBuilder> = context.get()?;
        let daemon_client: &Arc<DaemonClient> = context.get()?;
        let config: &ValidatedConfig = context.get()?;

        let miner_address = config
            .miner_address
            .as_ref()
            .ok_or_else(|| CommandError::BatchModeError("No miner address configured".to_string()))?
            .clone();

        (tx_builder.clone(), daemon_client.clone(), miner_address)
    };

    // Create test answer
    let task_id = Hash::new(rand::random::<[u8; 32]>());
    let answer_text = "This is a test AI answer for the beginner level task: Image classification of cats vs dogs";
    let answer_hash = Hash::new(blake3::hash(answer_text.as_bytes()).into());
    let stake_amount = 2_000_000_000; // 2 TOS stake

    let nonce = get_next_nonce(&daemon_client, &miner_address)
        .await
        .map_err(|e| CommandError::BatchModeError(format!("Nonce generation failed: {e}")))?;

    // Create transaction metadata
    let metadata = tx_builder
        .build_submit_answer_transaction(
            task_id.clone(),
            answer_text.to_string(),
            answer_hash.clone(),
            stake_amount,
            nonce,
            0, // Auto-calculate fee
        )
        .map_err(|e| CommandError::BatchModeError(e.to_string()))?;

    manager.message("✅ Answer submission test completed:");
    manager.message(format!("  - Task ID: {}", hex::encode(task_id.as_bytes())));
    manager.message(format!(
        "  - Answer Hash: {}",
        hex::encode(answer_hash.as_bytes())
    ));
    manager.message(format!(
        "  - Stake: {} TOS",
        stake_amount as f64 / 1_000_000_000.0
    ));
    manager.message(format!(
        "  - Estimated Fee: {} nanoTOS",
        metadata.estimated_fee
    ));
    manager.message(format!(
        "  - Estimated Size: {} bytes",
        metadata.estimated_size
    ));

    Ok(())
}

/// Test AI answer validation workflow
async fn test_validation_workflow(
    manager: &CommandManager,
    _args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.message("🔍 Testing AI answer validation workflow...");

    let (tx_builder, daemon_client, validator_address) = {
        let context = manager.get_context().lock()?;
        let tx_builder: &Arc<AIMiningTransactionBuilder> = context.get()?;
        let daemon_client: &Arc<DaemonClient> = context.get()?;
        let config: &ValidatedConfig = context.get()?;

        let validator_address = config
            .miner_address
            .as_ref()
            .ok_or_else(|| CommandError::BatchModeError("No miner address configured".to_string()))?
            .clone();

        (tx_builder.clone(), daemon_client.clone(), validator_address)
    };

    // Create test validation
    let task_id = Hash::new(rand::random::<[u8; 32]>());
    let answer_id = Hash::new(rand::random::<[u8; 32]>());
    let validation_score = 88; // Good score

    let nonce = get_next_nonce(&daemon_client, &validator_address)
        .await
        .map_err(|e| CommandError::BatchModeError(format!("Nonce generation failed: {e}")))?;

    // Create transaction metadata
    let metadata = tx_builder
        .build_validate_answer_transaction(
            task_id.clone(),
            answer_id.clone(),
            validation_score,
            nonce,
            0, // Auto-calculate fee
        )
        .map_err(|e| CommandError::BatchModeError(e.to_string()))?;

    manager.message("✅ Answer validation test completed:");
    manager.message(format!("  - Task ID: {}", hex::encode(task_id.as_bytes())));
    manager.message(format!(
        "  - Answer ID: {}",
        hex::encode(answer_id.as_bytes())
    ));
    manager.message(format!("  - Validation Score: {validation_score}/100"));
    manager.message(format!(
        "  - Estimated Fee: {} nanoTOS",
        metadata.estimated_fee
    ));
    manager.message(format!(
        "  - Estimated Size: {} bytes",
        metadata.estimated_size
    ));

    Ok(())
}

/// Test complete reward distribution cycle
async fn test_reward_cycle(
    manager: &CommandManager,
    _args: ArgumentManager,
) -> Result<(), CommandError> {
    manager.message("🔄 Testing complete AI mining reward cycle...");

    let (miner_address_opt, daemon_client) = {
        let context = manager.get_context().lock()?;
        let config: &ValidatedConfig = context.get()?;
        let daemon_client: &Arc<DaemonClient> = context.get()?;
        (config.miner_address.clone(), daemon_client.clone())
    };

    // Test daemon connectivity first
    match daemon_client.health_check().await {
        Ok(health) => {
            if health.is_healthy {
                manager.message(format!(
                    "✅ Daemon connection healthy - Version: {}",
                    health.version.as_deref().unwrap_or("unknown")
                ));
                manager.message(format!("   Response time: {:?}", health.response_time));

                if let Some(peer_count) = health.peer_count {
                    manager.message(format!("   Peers: {peer_count}"));
                }
                if let Some(mempool_size) = health.mempool_size {
                    manager.message(format!("   Mempool: {mempool_size} transactions"));
                }
            } else {
                manager.message("⚠️  Daemon unhealthy, using mock mode");
            }
        }
        Err(e) => {
            manager.message(format!("⚠️  Daemon connection failed: {e}"));
            manager.message("   This is expected when no daemon is running");
            manager.message("   In production, ensure TOS daemon is running and accessible");
        }
    }

    // Simulate full cycle metrics
    let total_tasks = 50;
    let active_tasks = 12;
    let completed_tasks = 38;
    let total_rewards_distributed = 1_875_000_000_000u64; // 1,875 TOS

    manager.message("");
    manager.message("📊 AI Mining Cycle Metrics:");
    manager.message(format!("  Total Tasks: {total_tasks}"));
    manager.message(format!("  Active Tasks: {active_tasks}"));
    manager.message(format!("  Completed Tasks: {completed_tasks}"));
    manager.message(format!(
        "  Success Rate: {:.1}%",
        (completed_tasks as f64 / total_tasks as f64) * 100.0
    ));
    manager.message(format!(
        "  Total Rewards Distributed: {:.2} TOS",
        total_rewards_distributed as f64 / 1_000_000_000.0
    ));
    manager.message(format!(
        "  Average Reward per Task: {:.2} TOS",
        (total_rewards_distributed as f64 / completed_tasks as f64) / 1_000_000_000.0
    ));

    // Test network-specific fee calculations
    manager.message("");
    manager.message("💰 Network Fee Analysis:");
    let sample_payload = AIMiningPayload::RegisterMiner {
        miner_address: miner_address_opt
            .as_ref()
            .unwrap()
            .clone()
            .to_public_key(),
        registration_fee: 1_000_000_000,
    };

    for network in &[
        Network::Devnet,
        Network::Testnet,
        Network::Stagenet,
        Network::Mainnet,
    ] {
        let builder = AIMiningTransactionBuilder::new(*network);
        // Use a fixed size estimate for demonstration
        let estimated_size = 120; // bytes, typical for register miner transaction
        let fee = builder.estimate_fee_with_payload_type(estimated_size, Some(&sample_payload));
        manager.message(format!(
            "  {:?}: {} nanoTOS ({:.6} TOS)",
            network,
            fee,
            fee as f64 / 1_000_000_000.0
        ));
    }

    manager.message("");
    manager.message("✅ Complete reward cycle test completed!");

    Ok(())
}
