use std::ops::ControlFlow;

use argon2::{Algorithm, Argon2, Params, Version};
#[cfg(feature = "cli")]
use clap::Parser;
use lazy_static::lazy_static;
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tos_common::{
    config::VERSION, crypto::ecdlp, network::Network, utils::detect_available_parallelism,
};

use crate::precomputed_tables;
#[cfg(feature = "cli")]
use tos_common::prompt::{default_logs_datetime_format, LogLevel, ModuleConfig};

pub const DIR_PATH: &str = "wallets/";
pub const XSWD_BIND_ADDRESS: &str = "0.0.0.0:44325";
pub const PASSWORD_HASH_SIZE: usize = 32;
pub const SALT_SIZE: usize = 32;
pub const KEY_SIZE: usize = 32;

// daemon address by default when no specified
pub const DEFAULT_DAEMON_ADDRESS: &str = "http://127.0.0.1:8080";
// Auto reconnect interval in seconds for Network Handler
pub const AUTO_RECONNECT_INTERVAL: u64 = 5;

lazy_static! {
    pub static ref PASSWORD_ALGORITHM: Argon2<'static> = {
        // 15 MB, 16 iterations
        let params = Params::new(15 * 1000, 16, 1, Some(PASSWORD_HASH_SIZE)).unwrap();
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
    };
}

// This struct is used to configure the RPC Server
// In case we want to enable it instead of starting
// the XSWD Server
#[cfg(all(feature = "api_server", feature = "cli"))]
#[derive(Debug, Clone, clap::Args, Serialize, Deserialize)]
pub struct RPCConfig {
    /// RPC Server bind address
    #[clap(long)]
    pub rpc_bind_address: Option<String>,
    /// username for RPC authentication
    #[clap(long)]
    pub rpc_username: Option<String>,
    /// password for RPC authentication
    #[clap(long)]
    pub rpc_password: Option<String>,
    /// Number of threads to use for the RPC Server
    #[clap(long)]
    pub rpc_threads: Option<usize>,
}

// Functions Helpers
fn default_daemon_address() -> String {
    DEFAULT_DAEMON_ADDRESS.to_owned()
}

fn default_precomputed_tables_l1() -> usize {
    precomputed_tables::L1_FULL
}

fn default_log_filename() -> String {
    String::from("tos-wallet.log")
}

fn default_logs_path() -> String {
    String::from("logs/")
}

#[cfg(feature = "cli")]
#[derive(Debug, Clone, clap::Args, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Daemon address to use
    #[cfg(feature = "network_handler")]
    #[clap(long, default_value_t = String::from(DEFAULT_DAEMON_ADDRESS))]
    #[serde(default = "default_daemon_address")]
    pub daemon_address: String,
    /// Disable online mode
    #[cfg(feature = "network_handler")]
    #[clap(long)]
    pub offline_mode: bool,
}

#[cfg(feature = "cli")]
#[derive(Debug, Clone, clap::Args, Serialize, Deserialize)]
pub struct PrecomputedTablesConfig {
    /// L1 size for precomputed tables
    /// By default, it is set to 26 (L1_FULL)
    /// At each increment of 1, the size of the table is doubled
    /// L1_FULL = 26, L1_MEDIUM = 18, L1_LOW = 13
    #[clap(long, default_value_t = precomputed_tables::L1_FULL)]
    #[serde(default = "default_precomputed_tables_l1")]
    pub precomputed_tables_l1: usize,
    /// Set the path to use for precomputed tables
    ///
    /// By default, it will be from current directory.
    #[clap(long)]
    pub precomputed_tables_path: Option<String>,
}

#[cfg(feature = "cli")]
#[derive(Debug, Clone, clap::Args, Serialize, Deserialize)]
pub struct LogConfig {
    /// Set log level
    #[clap(long, value_enum, default_value_t)]
    #[serde(default)]
    pub log_level: LogLevel,
    /// Set file log level
    /// By default, it will be the same as log level
    #[clap(long, value_enum)]
    pub file_log_level: Option<LogLevel>,
    /// Disable the log file
    #[clap(long)]
    #[serde(default)]
    pub disable_file_logging: bool,
    /// Disable the log filename date based
    /// If disabled, the log file will be named tos-wallet.log instead of YYYY-MM-DD.tos-wallet.log
    #[clap(long)]
    #[serde(default)]
    pub disable_file_log_date_based: bool,
    /// Enable the log file auto compression
    /// If enabled, the log file will be compressed every day
    /// This will only work if the log file is enabled
    #[clap(long)]
    #[serde(default)]
    pub auto_compress_logs: bool,
    /// Disable the usage of colors in log
    #[clap(long)]
    #[serde(default)]
    pub disable_log_color: bool,
    /// Disable terminal interactive mode
    /// You will not be able to write CLI commands in it or to have an updated prompt
    #[clap(long)]
    #[serde(default)]
    pub disable_interactive_mode: bool,
    /// Log filename
    ///
    /// By default filename is tos-wallet.log.
    /// File will be stored in logs directory, this is only the filename, not the full path.
    /// Log file is rotated every day and has the format YYYY-MM-DD.tos-wallet.log.
    #[clap(long, default_value_t = default_log_filename())]
    #[serde(default = "default_log_filename")]
    pub filename_log: String,
    /// Logs directory
    ///
    /// By default it will be logs/ of the current directory.
    /// It must end with a / to be a valid folder.
    #[clap(long, default_value_t = default_logs_path())]
    #[serde(default = "default_logs_path")]
    pub logs_path: String,
    /// Module configuration for logs
    #[clap(long)]
    #[serde(default)]
    pub logs_modules: Vec<ModuleConfig>,
    /// Disable the ascii art at startup
    #[clap(long)]
    #[serde(default)]
    pub disable_ascii_art: bool,
    /// Change the datetime format used by the logger
    #[clap(long, default_value_t = default_logs_datetime_format())]
    #[serde(default = "default_logs_datetime_format")]
    pub datetime_format: String,
}

#[cfg(feature = "cli")]
#[derive(Parser, Serialize, Deserialize, Clone)]
#[clap(
    version = VERSION,
    about = "TOS Wallet - Manage your TOS cryptocurrency wallet from command line",
    long_about = r#"TOS Wallet - Non-Interactive Command Line Interface

IMPORTANT: This wallet operates in NON-INTERACTIVE mode by default for automation and AI tools.
You do NOT need to use interactive prompts. All commands can be executed with command-line arguments.

═══════════════════════════════════════════════════════════════════════════════
QUICK START GUIDE - NON-INTERACTIVE MODE
═══════════════════════════════════════════════════════════════════════════════

1. CREATE A NEW WALLET (non-interactive):
   ./tos_wallet --network devnet --wallet-path my_wallet --password mypass123 --exec "display_address"

   This will:
   - Automatically create a new wallet at ./wallets/my_wallet/ if it doesn't exist
   - Encrypt it with password "mypass123"
   - Display the wallet address
   - Exit immediately

   To also see the seed phrase:
   ./tos_wallet --network devnet --wallet-path my_wallet --password mypass123 --exec "seed"

2. OPEN EXISTING WALLET AND SHOW ADDRESS:
   ./tos_wallet --network devnet --wallet-path my_wallet --password mypass123 --exec "address"

3. GET WALLET BALANCE:
   ./tos_wallet --network devnet --daemon-address http://127.0.0.1:8080 \
       --wallet-path my_wallet --password mypass123 --exec "balance"

4. SEND TRANSACTION (non-interactive):
   ./tos_wallet --network devnet --daemon-address http://127.0.0.1:8080 \
       --wallet-path my_wallet --password mypass123 \
       --exec "transfer <asset> <recipient_address> <amount>"

   Example:
   ./tos_wallet --network devnet --daemon-address http://127.0.0.1:8080 \
       --wallet-path my_wallet --password mypass123 \
       --exec "transfer TOS tst1yp0hc5z0csf2jk2ze9tjjxkjg8gawt2upltksyegffmudm29z38qqrkvqzk 1.5"

5. RESTORE WALLET FROM SEED:
   ./tos_wallet --network devnet --wallet-path restored_wallet --password newpass456 \
       --seed "word1 word2 word3 ... word24" --exec "display_address"

   Note: The --seed flag restores from an existing seed phrase (24 words)

═══════════════════════════════════════════════════════════════════════════════
PASSWORD OPTIONS (pick one):
═══════════════════════════════════════════════════════════════════════════════

Option 1: Direct password (quick, less secure):
  --password mypassword

Option 2: Password from environment variable (more secure):
  export TOS_WALLET_PASSWORD="mypassword"
  ./tos_wallet --password-from-env --wallet-path my_wallet --exec "balance"

Option 3: Password from file (most secure):
  echo "mypassword" > password.txt
  chmod 600 password.txt
  ./tos_wallet --password-file password.txt --wallet-path my_wallet --exec "balance"

═══════════════════════════════════════════════════════════════════════════════
AVAILABLE COMMANDS (use with --exec):
═══════════════════════════════════════════════════════════════════════════════

Wallet Management:
  display_address           - Show wallet address (auto-creates wallet if needed)
  address                   - Alias for display_address
  seed                      - Display seed phrase (requires password)
  balance                   - Show wallet balance (requires --daemon-address)

Transactions:
  transfer <asset> <address> <amount>      - Send asset to address (asset can be 'TOS' or asset hash)
  history                                  - Show transaction history
  nonce                                    - Show current nonce

Asset Management:
  list_assets               - List tracked assets
  track_asset <hash>        - Track a new asset

Advanced:
  freeze <amount> <days>    - Freeze TOS to generate energy
  unfreeze <tx_hash>        - Unfreeze previously frozen TOS
  multisig                  - Manage multisig operations

═══════════════════════════════════════════════════════════════════════════════
NETWORK OPTIONS:
═══════════════════════════════════════════════════════════════════════════════

--network devnet          - Development network (default daemon: 127.0.0.1:8080)
--network testnet         - Test network
--network mainnet         - Main network (default)

--daemon-address <url>    - Daemon RPC endpoint (default: http://127.0.0.1:8080)
--offline-mode            - Work without connecting to daemon (limited functionality)

═══════════════════════════════════════════════════════════════════════════════
EXAMPLES FOR AI TOOLS:
═══════════════════════════════════════════════════════════════════════════════

# Create wallet, fund it, and send transaction (complete flow):

# Step 1: Create new wallet (auto-creates on first command)
./tos_wallet --network devnet --wallet-path sender_wallet --password pass123 --exec "display_address"

# Step 2: Get wallet address (to receive funds)
./tos_wallet --network devnet --wallet-path sender_wallet --password pass123 --exec "address"

# Step 3: Check balance after mining/receiving funds
./tos_wallet --network devnet --daemon-address http://127.0.0.1:8080 \
    --wallet-path sender_wallet --password pass123 --exec "balance"

# Step 4: Send 4 transactions to trigger parallel execution (devnet threshold = 4)
for i in {1..4}; do
    ./tos_wallet --network devnet --daemon-address http://127.0.0.1:8080 \
        --wallet-path sender_wallet --password pass123 \
        --exec "transfer TOS tst1yp0hc5z0csf2jk2ze9tjjxkjg8gawt2upltksyegffmudm29z38qqrkvqzk 1.0"
    sleep 0.3
done

# Step 5: Verify recipient balance
./tos_wallet --network devnet --daemon-address http://127.0.0.1:8080 \
    --wallet-path recipient_wallet --password pass456 --exec "balance"

═══════════════════════════════════════════════════════════════════════════════
BATCH MODE (JSON):
═══════════════════════════════════════════════════════════════════════════════

Create a JSON file (transfer.json):
{
  "command": "transfer",
  "params": {
    "asset": "TOS",
    "address": "tst1yp0hc5z0csf2jk2ze9tjjxkjg8gawt2upltksyegffmudm29z38qqrkvqzk",
    "amount": "1.0"
  }
}

Execute:
./tos_wallet --network devnet --daemon-address http://127.0.0.1:8080 \
    --wallet-path my_wallet --password mypass123 --json-file transfer.json

═══════════════════════════════════════════════════════════════════════════════

For interactive mode (with prompts), add --interactive flag.
For more help on specific commands, use: help <command> in interactive mode.
"#
)]
#[command(styles = tos_common::get_cli_styles())]
pub struct Config {
    /// RPC Server configuration
    #[cfg(feature = "api_server")]
    #[structopt(flatten)]
    pub rpc: RPCConfig,
    /// Network Configuration
    #[structopt(flatten)]
    pub network_handler: NetworkConfig,
    /// Precopmuted tables configuration
    #[structopt(flatten)]
    pub precomputed_tables: PrecomputedTablesConfig,
    /// Log configuration
    #[structopt(flatten)]
    pub log: LogConfig,
    /// Set the path for wallet storage to open/create a wallet at this location
    #[clap(long)]
    pub wallet_path: Option<String>,
    /// Password used to open wallet
    #[clap(long)]
    pub password: Option<String>,
    /// Restore wallet using seed
    #[clap(long)]
    pub seed: Option<String>,
    /// How many threads we want to use
    /// during ciphertext decryption
    #[clap(long, default_value_t = detect_available_parallelism())]
    #[serde(default = "detect_available_parallelism")]
    pub n_decryption_threads: usize,
    /// Concurrency configuration for Network Handler
    #[clap(long, default_value_t = detect_available_parallelism())]
    #[serde(default = "detect_available_parallelism")]
    pub network_concurrency: usize,
    /// Network selected for chain
    #[clap(long, value_enum, default_value_t = Network::Mainnet)]
    #[serde(default)]
    pub network: Network,
    /// XSWD Server configuration
    #[cfg(feature = "api_server")]
    #[clap(long)]
    #[serde(default)]
    pub enable_xswd: bool,
    /// Disable the history scan
    /// This will prevent syncing old TXs/blocks
    /// Only blocks / transactions caught by the network handler will be stored, not the old ones
    #[clap(long)]
    #[serde(default)]
    pub disable_history_scan: bool,
    /// Enable light wallet mode (no blockchain synchronization)
    /// Light mode queries nonce/balance/reference on-demand from daemon, enabling instant startup
    /// Trade-off: Transaction history is not available locally
    #[clap(long)]
    #[serde(default)]
    pub light_mode: bool,
    /// Force the wallet to use a stable balance only during transactions creation.
    /// This will prevent the wallet to use unstable balance and prevent any orphaned transaction due to DAG reorg.
    /// This is only working if the wallet is in online mode.
    #[clap(long)]
    #[serde(default)]
    pub force_stable_balance: bool,
    /// JSON File to load the configuration from
    #[clap(long)]
    #[serde(skip)]
    #[serde(default)]
    pub config_file: Option<String>,
    /// Generate the template at the `config_file` path
    #[clap(long)]
    #[serde(skip)]
    #[serde(default)]
    pub generate_config_template: bool,
    /// Execute a command and exit (similar to geth --exec)
    #[clap(long)]
    #[serde(skip)]
    #[serde(default)]
    pub exec: Option<String>,
    /// JSON string containing batch command and parameters
    #[clap(long)]
    #[serde(skip)]
    #[serde(default)]
    pub json: Option<String>,
    /// JSON file path containing batch command and parameters
    #[clap(long)]
    #[serde(skip)]
    #[serde(default)]
    pub json_file: Option<String>,
    /// Enable interactive mode (prompt for missing arguments)
    /// Default: false (pure command mode)
    #[clap(long)]
    #[serde(default)]
    pub interactive: bool,
    /// Read password from environment variable TOS_WALLET_PASSWORD
    #[clap(long)]
    #[serde(default)]
    pub password_from_env: bool,
    /// Read password from file (more secure than --password)
    #[clap(long)]
    pub password_file: Option<String>,
}

#[cfg(feature = "cli")]
impl Config {
    /// Check if we're in exec mode (--exec, --json, or --json-file)
    pub fn is_exec_mode(&self) -> bool {
        self.exec.is_some() || self.json.is_some() || self.json_file.is_some()
    }

    /// Check if we're in interactive mode
    pub fn is_interactive_mode(&self) -> bool {
        self.interactive && !self.is_exec_mode()
    }

    /// Check if we're in command mode (default)
    pub fn is_command_mode(&self) -> bool {
        !self.is_interactive_mode()
    }

    /// Get the command to execute (from --exec)
    pub fn get_exec_command(&self) -> Option<&String> {
        self.exec.as_ref()
    }
}

/// JSON batch configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonBatchConfig {
    /// Command to execute
    pub command: String,
    /// Wallet path (optional, can use CLI parameter)
    pub wallet_path: Option<String>,
    /// Password (optional, can use CLI parameter)
    pub password: Option<String>,
    /// Command parameters
    pub params: HashMap<String, serde_json::Value>,
}

/// This struct is used to log the progress of the table generation
pub struct LogProgressTableGenerationReportFunction;

impl ecdlp::ProgressTableGenerationReportFunction for LogProgressTableGenerationReportFunction {
    fn report(&self, progress: f64, step: ecdlp::ReportStep) -> ControlFlow<()> {
        if log::log_enabled!(log::Level::Info) {
            info!("Progress: {:.2}% on step {:?}", progress * 100.0, step);
        }
        ControlFlow::Continue(())
    }
}
