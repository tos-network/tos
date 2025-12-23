use std::{
    collections::HashMap,
    fmt::Display,
    future::Future,
    pin::Pin,
    rc::Rc,
    str::FromStr,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use crate::{async_handler, config::VERSION, context::Context};

use super::{argument::*, LogLevel, ShareablePrompt};
use anyhow::Error;
use log::{error, info, warn};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("Expected a command name")]
    ExpectedCommandName,
    #[error("Command was not found")]
    CommandNotFound,
    #[error("Expected required argument {}", _0)]
    ExpectedRequiredArg(String), // arg name
    #[error("Too many arguments")]
    TooManyArguments,
    #[error(transparent)]
    ArgError(#[from] ArgError),
    #[error("Invalid argument: {}", _0)]
    InvalidArgument(String),
    #[error("Exit command was called")]
    Exit,
    #[error("No data was set in command manager")]
    NoData,
    #[error("No prompt was set in command manager")]
    NoPrompt,
    #[error(transparent)]
    Any(#[from] Error),
    #[error("Poison Error: {}", _0)]
    PoisonError(String),
    #[error(
        "Missing required argument '{}' in batch mode. Provide it as a positional argument in --exec command.",
        _0
    )]
    MissingArgument(String),
    #[error("Batch mode error: {}", _0)]
    BatchModeError(String),
    #[error("Missing required argument '{arg}' in command mode.\nUsage: {usage}\nUse --interactive to enable prompts.")]
    MissingRequiredArgument { arg: String, usage: String },
    #[error("Missing required argument '{arg}' in command mode.\n\nUsage: {usage}\n\nExample:\n  {example}\n\nUse --interactive to enable prompts.")]
    MissingRequiredArgumentWithExample {
        arg: String,
        usage: String,
        example: String,
    },
    #[error("Invalid {param}: {message}\n\nUsage: {usage}\n\nExample:\n  {example}")]
    InvalidParameterWithExample {
        param: String,
        message: String,
        usage: String,
        example: String,
    },
    #[error("Missing confirmation for destructive operation.\nAdd '--confirm true' to proceed or '--confirm false' to cancel explicitly.")]
    MissingConfirmation,
    #[error("Password required in command mode.\nUse: --password <pwd> OR --password-file <path> OR --password-from-env OR set TOS_WALLET_PASSWORD environment variable.")]
    PasswordRequired,
}

impl<T> From<PoisonError<T>> for CommandError {
    fn from(err: PoisonError<T>) -> Self {
        Self::PoisonError(format!("{}", err))
    }
}

pub type SyncCommandCallback = fn(&CommandManager, ArgumentManager) -> Result<(), CommandError>;
pub type AsyncCommandCallback = fn(
    &'_ CommandManager,
    ArgumentManager,
) -> Pin<Box<dyn Future<Output = Result<(), CommandError>> + '_>>;

pub enum CommandHandler {
    Sync(SyncCommandCallback),
    Async(AsyncCommandCallback),
}

pub struct Command {
    name: String,
    description: String,
    required_args: Vec<Arg>,
    optional_args: Vec<Arg>,
    callback: CommandHandler,
}

impl Command {
    pub fn new(name: &str, description: &str, callback: CommandHandler) -> Self {
        Self {
            name: name.to_owned(),
            description: description.to_owned(),
            required_args: Vec::new(),
            optional_args: Vec::new(),
            callback,
        }
    }

    pub fn with_optional_arguments(
        name: &str,
        description: &str,
        optional_args: Vec<Arg>,
        callback: CommandHandler,
    ) -> Self {
        Self {
            name: name.to_owned(),
            description: description.to_owned(),
            required_args: Vec::new(),
            optional_args,
            callback,
        }
    }

    pub fn with_required_arguments(
        name: &str,
        description: &str,
        required_args: Vec<Arg>,
        callback: CommandHandler,
    ) -> Self {
        Self {
            name: name.to_owned(),
            description: description.to_owned(),
            required_args,
            optional_args: Vec::new(),
            callback,
        }
    }

    pub fn with_arguments(
        name: &str,
        description: &str,
        required_args: Vec<Arg>,
        optional_args: Vec<Arg>,
        callback: CommandHandler,
    ) -> Self {
        Self {
            name: name.to_owned(),
            description: description.to_owned(),
            required_args,
            optional_args,
            callback,
        }
    }

    pub async fn execute(
        &self,
        manager: &CommandManager,
        values: ArgumentManager,
    ) -> Result<(), CommandError> {
        match &self.callback {
            CommandHandler::Sync(handler) => handler(manager, values),
            CommandHandler::Async(handler) => handler(manager, values).await,
        }
    }

    pub fn get_name(&self) -> &String {
        &self.name
    }

    pub fn get_description(&self) -> &String {
        &self.description
    }

    pub fn get_required_args(&self) -> &Vec<Arg> {
        &self.required_args
    }

    pub fn get_optional_args(&self) -> &Vec<Arg> {
        &self.optional_args
    }

    pub fn get_usage(&self) -> String {
        let required_args: Vec<String> = self
            .get_required_args()
            .iter()
            .map(|arg| format!("<{}>", arg.get_name()))
            .collect();

        let optional_args: Vec<String> = self
            .get_optional_args()
            .iter()
            .map(|arg| format!("[{}]", arg.get_name()))
            .collect();

        format!(
            "{} {}{}",
            self.get_name(),
            required_args.join(" "),
            optional_args.join(" ")
        )
    }

    /// Get detailed help information for this command
    pub fn get_detailed_help(&self) -> String {
        let mut help = String::new();

        // Command name and description
        help.push_str(&format!("{}\n", self.name));
        help.push_str(&format!("{}\n\n", self.description));

        // Usage
        help.push_str("USAGE:\n");
        help.push_str(&format!("  {}\n\n", self.get_usage()));

        // Required arguments
        if !self.required_args.is_empty() {
            help.push_str("REQUIRED ARGUMENTS:\n");
            for arg in &self.required_args {
                help.push_str(&format!(
                    "  <{}>  {}\n",
                    arg.get_name(),
                    arg.get_description()
                ));
            }
            help.push('\n');
        }

        // Optional arguments
        if !self.optional_args.is_empty() {
            help.push_str("OPTIONAL ARGUMENTS:\n");
            for arg in &self.optional_args {
                help.push_str(&format!(
                    "  [{}]  {}\n",
                    arg.get_name(),
                    arg.get_description()
                ));
            }
            help.push('\n');
        }

        help
    }
}

// We use Mutex from std instead of tokio so we can use it in sync code too
pub struct CommandManager {
    commands: Mutex<Vec<Rc<Command>>>,
    context: Mutex<Context>,
    prompt: ShareablePrompt,
    running_since: Instant,
    // Note: batch_mode removed - wallet now always operates in batch mode
}

impl CommandManager {
    pub fn with_context(context: Context, prompt: ShareablePrompt) -> Self {
        Self {
            commands: Mutex::new(Vec::new()),
            context: Mutex::new(context),
            prompt,
            running_since: Instant::now(),
        }
    }

    pub fn new(prompt: ShareablePrompt) -> Self {
        Self::with_context(Context::new(), prompt)
    }

    /// Create CommandManager (always in batch mode)
    /// The exec_mode parameter is kept for API compatibility but ignored
    #[allow(unused_variables)]
    pub fn with_batch_mode(context: Context, prompt: ShareablePrompt, exec_mode: bool) -> Self {
        Self::with_context(context, prompt)
    }

    /// Create CommandManager (always in batch mode)
    /// The exec_mode parameter is kept for API compatibility but ignored
    #[allow(unused_variables)]
    pub fn new_with_batch_mode(prompt: ShareablePrompt, exec_mode: bool) -> Self {
        Self::new(prompt)
    }

    // Register default commands:
    // - help
    // - version
    // - exit
    // - set_log_level
    pub fn register_default_commands(&self) -> Result<(), CommandError> {
        self.add_command(Command::with_optional_arguments(
            "help",
            "Show this help",
            vec![Arg::new(
                "command",
                ArgType::String,
                "Command name to get help for",
            )],
            CommandHandler::Async(async_handler!(help)),
        ))?;
        self.add_command(Command::new(
            "version",
            "Show the current version",
            CommandHandler::Sync(version),
        ))?;
        self.add_command(Command::new(
            "exit",
            "Shutdown the application",
            CommandHandler::Sync(exit),
        ))?;
        self.add_command(Command::with_required_arguments(
            "set_log_level",
            "Set the log level",
            vec![Arg::new(
                "level",
                ArgType::String,
                "Log level (off/error/warn/info/debug/trace)",
            )],
            CommandHandler::Sync(set_log_level),
        ))?;

        Ok(())
    }

    pub fn store_in_context<T: Send + Sync + 'static>(&self, data: T) -> Result<(), CommandError> {
        let mut context = self.context.lock()?;
        context.store(data);
        Ok(())
    }

    pub fn remove_from_context<T: Send + Sync + 'static>(&self) -> Result<(), CommandError> {
        let mut context = self.context.lock()?;
        context.remove::<T>();
        Ok(())
    }

    pub fn get_context(&self) -> &Mutex<Context> {
        &self.context
    }

    pub fn get_prompt(&self) -> &ShareablePrompt {
        &self.prompt
    }

    pub fn add_command(&self, command: Command) -> Result<(), CommandError> {
        let mut commands = self.commands.lock()?;
        commands.push(Rc::new(command));
        Ok(())
    }

    pub fn remove_all_commands(&self) -> Result<(), CommandError> {
        let mut commands = self.commands.lock()?;
        commands.clear();
        Ok(())
    }

    pub fn remove_command(&self, command_name: &str) -> Result<bool, CommandError> {
        let mut commands = self.commands.lock()?;
        if let Some(index) = commands
            .iter()
            .position(|cmd| cmd.get_name() == command_name)
        {
            commands.remove(index);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn get_commands(&self) -> &Mutex<Vec<Rc<Command>>> {
        &self.commands
    }

    /// Get detailed help for a specific command
    pub fn get_command_help(&self, command_name: &str) -> Result<String, CommandError> {
        let commands = self.commands.lock()?;
        let command = commands
            .iter()
            .find(|cmd| cmd.get_name() == command_name)
            .ok_or(CommandError::CommandNotFound)?;

        Ok(command.get_detailed_help())
    }

    /// Handle command from JSON parameters
    pub async fn handle_json_command(
        &self,
        command_name: &str,
        json_params: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(), CommandError> {
        let command = {
            let commands = self.commands.lock()?;
            commands
                .iter()
                .find(|command| *command.get_name() == *command_name)
                .cloned()
                .ok_or(CommandError::CommandNotFound)?
        };

        // Create ArgumentManager from JSON params
        let arguments = ArgumentManager::from_json_params(json_params)
            .map_err(|e| CommandError::InvalidArgument(e.to_string()))?;

        // Validate batch parameters
        self.validate_batch_params(command_name, &arguments)?;

        command.execute(self, arguments).await
    }

    pub async fn handle_command(&self, value: String) -> Result<(), CommandError> {
        let mut command_split = value.split_whitespace();
        let command_name = command_split
            .next()
            .ok_or(CommandError::ExpectedCommandName)?;
        let command = {
            let commands = self.commands.lock()?;
            commands
                .iter()
                .find(|command| *command.get_name() == *command_name)
                .cloned()
                .ok_or(CommandError::CommandNotFound)?
        };
        let mut arguments: HashMap<String, ArgValue> = HashMap::new();
        for arg in command.get_required_args() {
            let arg_value = command_split
                .next()
                .ok_or_else(|| CommandError::ExpectedRequiredArg(arg.get_name().to_owned()))?;
            arguments.insert(arg.get_name().clone(), arg.get_type().to_value(arg_value)?);
        }

        // include all options args available
        for optional_arg in command.get_optional_args() {
            if let Some(arg_value) = command_split.next() {
                arguments.insert(
                    optional_arg.get_name().clone(),
                    optional_arg.get_type().to_value(arg_value)?,
                );
            } else {
                break;
            }
        }

        if command_split.next().is_some() {
            return Err(CommandError::TooManyArguments);
        }

        command.execute(self, ArgumentManager::new(arguments)).await
    }

    pub fn display_commands(&self) -> Result<(), CommandError> {
        let commands = self.commands.lock()?;
        self.message("Available commands:");
        for cmd in commands.iter() {
            self.message(format!("- {}: {}", cmd.get_name(), cmd.get_description()));
        }
        Ok(())
    }

    pub fn message<D: Display>(&self, message: D) {
        if log::log_enabled!(log::Level::Info) {
            info!("{message}");
        }
    }

    pub fn warn<D: Display>(&self, message: D) {
        if log::log_enabled!(log::Level::Warn) {
            warn!("{message}");
        }
    }

    pub fn error<D: Display>(&self, message: D) {
        if log::log_enabled!(log::Level::Error) {
            error!("{message}");
        }
    }

    pub fn running_since(&self) -> Duration {
        self.running_since.elapsed()
    }

    /// Require a parameter, throw error if missing (always in batch mode)
    pub fn require_param(
        &self,
        args: &ArgumentManager,
        param_name: &str,
    ) -> Result<(), CommandError> {
        if !args.has_argument(param_name) {
            return Err(CommandError::MissingArgument(param_name.to_string()));
        }
        Ok(())
    }

    /// Validate required parameters (always in batch mode)
    pub fn validate_batch_params(
        &self,
        command_name: &str,
        args: &ArgumentManager,
    ) -> Result<(), CommandError> {
        // Always validate - we're always in batch mode

        match command_name {
            "open" => {
                self.require_param(args, "name")?;
                self.require_param(args, "password")?;
            }
            "create" => {
                self.require_param(args, "name")?;
                self.require_param(args, "password")?;
            }
            "recover_seed" => {
                self.require_param(args, "name")?;
                self.require_param(args, "password")?;
                self.require_param(args, "seed")?;
            }
            "recover_private_key" => {
                self.require_param(args, "name")?;
                self.require_param(args, "password")?;
                self.require_param(args, "private_key")?;
            }
            "transfer" => {
                self.require_param(args, "address")?;
                self.require_param(args, "amount")?;
                self.require_param(args, "asset")?;
            }
            "transfer_all" => {
                self.require_param(args, "address")?;
                self.require_param(args, "asset")?;
            }
            "burn" => {
                self.require_param(args, "asset")?;
                self.require_param(args, "amount")?;
            }
            "change_password" => {
                self.require_param(args, "old_password")?;
                self.require_param(args, "new_password")?;
            }
            "export_transactions" => {
                self.require_param(args, "filename")?;
            }
            "freeze_tos" => {
                self.require_param(args, "amount")?;
                self.require_param(args, "duration")?;
                self.require_param(args, "confirm")?;
            }
            "unfreeze_tos" => {
                self.require_param(args, "amount")?;
                self.require_param(args, "confirm")?;
            }
            "set_asset_name" => {
                self.require_param(args, "asset")?;
                self.require_param(args, "name")?;
            }
            "start_rpc_server" => {
                self.require_param(args, "bind_address")?;
                self.require_param(args, "username")?;
                self.require_param(args, "password")?;
            }
            "multisig_sign" => {
                self.require_param(args, "tx_hash")?;
            }
            "seed" => {
                self.require_param(args, "password")?;
            }
            "set_nonce" => {
                self.require_param(args, "nonce")?;
            }
            "track_asset" => {
                self.require_param(args, "asset")?;
            }
            "untrack_asset" => {
                self.require_param(args, "asset")?;
            }
            "set_tx_version" => {
                self.require_param(args, "version")?;
            }
            _ => {}
        }
        Ok(())
    }
}

async fn help(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    if args.has_argument("command") {
        let arg_value = args.get_value("command")?.to_string_value()?;
        let commands = manager.get_commands().lock()?;
        let cmd = commands
            .iter()
            .find(|command| *command.get_name() == *arg_value)
            .ok_or(CommandError::CommandNotFound)?;

        // Display command name and description
        manager.message(format!("Command: {}", cmd.get_name()));
        manager.message(format!("Description: {}", cmd.get_description()));
        manager.message(format!("Usage: {}", cmd.get_usage()));

        // Display required arguments with descriptions
        if !cmd.get_required_args().is_empty() {
            manager.message("Required arguments:");
            for arg in cmd.get_required_args() {
                manager.message(format!("  <{}>  {}", arg.get_name(), arg.get_description()));
            }
        }

        // Display optional arguments with descriptions
        if !cmd.get_optional_args().is_empty() {
            manager.message("Optional arguments:");
            for arg in cmd.get_optional_args() {
                manager.message(format!("  [{}]  {}", arg.get_name(), arg.get_description()));
            }
        }
    } else {
        manager.display_commands()?;
        manager.message("See how to use a command using help <command>");
    }
    Ok(())
}

fn exit(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    manager.message("Stopping...");
    Err(CommandError::Exit)
}

fn version(manager: &CommandManager, _: ArgumentManager) -> Result<(), CommandError> {
    manager.message(format!("Version: {}", VERSION));
    Ok(())
}

fn set_log_level(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    let arg_value = args.get_value("level")?.to_string_value()?;
    let level =
        LogLevel::from_str(&arg_value).map_err(|e| CommandError::InvalidArgument(e.to_owned()))?;
    log::set_max_level(level.into());
    manager.message(format!("Log level set to {}", level));

    Ok(())
}
