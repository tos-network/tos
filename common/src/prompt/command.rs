use std::{collections::HashMap, pin::Pin, future::Future, fmt::Display, time::{Instant, Duration}, sync::{Mutex, PoisonError}, rc::Rc, str::FromStr};

use crate::{config::VERSION, async_handler, context::Context};

use super::{argument::*, ShareablePrompt, LogLevel};
use anyhow::Error;
use thiserror::Error;
use log::{info, warn, error};

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
    #[error("Missing required argument '{}' in batch mode. Use --{} <value>", _0, _0)]
    MissingArgument(String),
    #[error("Batch mode error: {}", _0)]
    BatchModeError(String)
}

impl<T> From<PoisonError<T>> for CommandError {
    fn from(err: PoisonError<T>) -> Self {
        Self::PoisonError(format!("{}", err))
    }
}

pub type SyncCommandCallback = fn(&CommandManager, ArgumentManager) -> Result<(), CommandError>;
pub type AsyncCommandCallback = fn(&'_ CommandManager, ArgumentManager) -> Pin<Box<dyn Future<Output = Result<(), CommandError>> + '_>>;

pub enum CommandHandler {
    Sync(SyncCommandCallback),
    Async(AsyncCommandCallback)
}

pub struct Command {
    name: String,
    description: String,
    required_args: Vec<Arg>,
    optional_args: Vec<Arg>,
    callback: CommandHandler
}

impl Command {
    pub fn new(name: &str, description: &str, callback: CommandHandler) -> Self {
        Self {
            name: name.to_owned(),
            description: description.to_owned(),
            required_args: Vec::new(),
            optional_args: Vec::new(),
            callback
        }
    }

    pub fn with_optional_arguments(name: &str, description: &str, optional_args: Vec<Arg>, callback: CommandHandler) -> Self {
        Self {
            name: name.to_owned(),
            description: description.to_owned(),
            required_args: Vec::new(),
            optional_args,
            callback
        }
    }

    pub fn with_required_arguments(name: &str, description: &str, required_args: Vec<Arg>, callback: CommandHandler) -> Self {
        Self {
            name: name.to_owned(),
            description: description.to_owned(),
            required_args,
            optional_args: Vec::new(),
            callback
        }
    }

    pub fn with_arguments(name: &str, description: &str, required_args: Vec<Arg>, optional_args: Vec<Arg>, callback: CommandHandler) -> Self {
        Self {
            name: name.to_owned(),
            description: description.to_owned(),
            required_args,
            optional_args,
            callback
        }
    }

    pub async fn execute(&self, manager: &CommandManager, values: ArgumentManager) -> Result<(), CommandError> {
        match &self.callback {
            CommandHandler::Sync(handler) => {
                handler(manager, values)
            },
            CommandHandler::Async(handler) => {
                handler(manager, values).await
            },
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
        let required_args: Vec<String> = self.get_required_args()
            .iter()
            .map(|arg| format!("<{}>", arg.get_name()))
            .collect();

        let optional_args: Vec<String> = self.get_optional_args()
            .iter()
            .map(|arg| format!("[{}]", arg.get_name()))
            .collect();

        format!("{} {}{}", self.get_name(), required_args.join(" "), optional_args.join(" "))
    }
}

// We use Mutex from std instead of tokio so we can use it in sync code too
pub struct CommandManager {
    commands: Mutex<Vec<Rc<Command>>>,
    context: Mutex<Context>,
    prompt: ShareablePrompt,
    running_since: Instant,
    batch_mode: bool,
}

impl CommandManager {
    pub fn with_context(context: Context, prompt: ShareablePrompt) -> Self {
        Self {
            commands: Mutex::new(Vec::new()),
            context: Mutex::new(context),
            prompt,
            running_since: Instant::now(),
            batch_mode: false,
        }
    }

    pub fn new(prompt: ShareablePrompt) -> Self {
        Self::with_context(Context::new(), prompt)
    }

    pub fn with_batch_mode(context: Context, prompt: ShareablePrompt, exec_mode: bool) -> Self {
        Self {
            commands: Mutex::new(Vec::new()),
            context: Mutex::new(context),
            prompt,
            running_since: Instant::now(),
            batch_mode: exec_mode,
        }
    }

    pub fn new_with_batch_mode(prompt: ShareablePrompt, exec_mode: bool) -> Self {
        Self::with_batch_mode(Context::new(), prompt, exec_mode)
    }

    pub fn is_batch_mode(&self) -> bool {
        self.batch_mode
    }

    // Register default commands:
    // - help
    // - version
    // - exit
    // - set_log_level
    pub fn register_default_commands(&self) -> Result<(), CommandError> {
        self.add_command(Command::with_optional_arguments("help", "Show this help", vec![Arg::new("command", ArgType::String)], CommandHandler::Async(async_handler!(help))))?;
        self.add_command(Command::new("version", "Show the current version", CommandHandler::Sync(version)))?;
        self.add_command(Command::new("exit", "Shutdown the application", CommandHandler::Sync(exit)))?;
        self.add_command(Command::with_required_arguments("set_log_level", "Set the log level", vec![Arg::new("level", ArgType::String)], CommandHandler::Sync(set_log_level)))?;

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
        if let Some(index) = commands.iter().position(|cmd| cmd.get_name() == command_name) {
            commands.remove(index);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn get_commands(&self) -> &Mutex<Vec<Rc<Command>>> {
        &self.commands
    }

    /// Handle command from JSON parameters
    pub async fn handle_json_command(&self, command_name: &str, json_params: &std::collections::HashMap<String, serde_json::Value>) -> Result<(), CommandError> {
        let command = {
            let commands = self.commands.lock()?;
            commands.iter().find(|command| *command.get_name() == *command_name).cloned().ok_or(CommandError::CommandNotFound)?
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
        let command_name = command_split.next().ok_or(CommandError::ExpectedCommandName)?;
        let command = {
            let commands = self.commands.lock()?;
            commands.iter().find(|command| *command.get_name() == *command_name).cloned().ok_or(CommandError::CommandNotFound)?
        };
        let mut arguments: HashMap<String, ArgValue> = HashMap::new();
        for arg in command.get_required_args() {
            let arg_value = command_split.next().ok_or_else(|| CommandError::ExpectedRequiredArg(arg.get_name().to_owned()))?;
            arguments.insert(arg.get_name().clone(), arg.get_type().to_value(arg_value)?);
        }

        // include all options args available
        for optional_arg in command.get_optional_args() {
            if let Some(arg_value) = command_split.next() {
                arguments.insert(optional_arg.get_name().clone(), optional_arg.get_type().to_value(arg_value)?);
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
        info!("{}", message);
    }

    pub fn warn<D: Display>(&self, message: D) {
        warn!("{}", message);
    }

    pub fn error<D: Display>(&self, message: D) {
        error!("{}", message);
    }

    pub fn running_since(&self) -> Duration {
        self.running_since.elapsed()
    }

    /// Require a parameter in batch mode, throw error if missing
    pub fn require_param(&self, args: &ArgumentManager, param_name: &str) -> Result<(), CommandError> {
        if self.batch_mode && !args.has_argument(param_name) {
            return Err(CommandError::MissingArgument(param_name.to_string()));
        }
        Ok(())
    }

    /// Validate required parameters for batch mode
    pub fn validate_batch_params(&self, command_name: &str, args: &ArgumentManager) -> Result<(), CommandError> {
        if !self.batch_mode {
            return Ok(());
        }

        match command_name {
            "open" => {
                self.require_param(args, "name")?;
                self.require_param(args, "password")?;
            },
            "create" => {
                self.require_param(args, "name")?;
                self.require_param(args, "password")?;
            },
            "recover_seed" => {
                self.require_param(args, "name")?;
                self.require_param(args, "password")?;
                self.require_param(args, "seed")?;
            },
            "recover_private_key" => {
                self.require_param(args, "name")?;
                self.require_param(args, "password")?;
                self.require_param(args, "private_key")?;
            },
            "transfer" => {
                self.require_param(args, "address")?;
                self.require_param(args, "amount")?;
                self.require_param(args, "asset")?;
            },
            "transfer_all" => {
                self.require_param(args, "address")?;
                self.require_param(args, "asset")?;
            },
            "burn" => {
                self.require_param(args, "asset")?;
                self.require_param(args, "amount")?;
            },
            "change_password" => {
                self.require_param(args, "old_password")?;
                self.require_param(args, "new_password")?;
            },
            "export_transactions" => {
                self.require_param(args, "filename")?;
            },
            "freeze_tos" => {
                self.require_param(args, "amount")?;
                self.require_param(args, "duration")?;
                self.require_param(args, "confirm")?;
            },
            "unfreeze_tos" => {
                self.require_param(args, "amount")?;
                self.require_param(args, "confirm")?;
            },
            "set_asset_name" => {
                self.require_param(args, "asset")?;
                self.require_param(args, "name")?;
            },
            "start_rpc_server" => {
                self.require_param(args, "bind_address")?;
                self.require_param(args, "username")?;
                self.require_param(args, "password")?;
            },
            "multisig_sign" => {
                self.require_param(args, "tx_hash")?;
            },
            "seed" => {
                self.require_param(args, "password")?;
            },
            "set_nonce" => {
                self.require_param(args, "nonce")?;
            },
            "track_asset" => {
                self.require_param(args, "asset")?;
            },
            "untrack_asset" => {
                self.require_param(args, "asset")?;
            },
            "set_tx_version" => {
                self.require_param(args, "version")?;
            },
            _ => {}
        }
        Ok(())
    }
}

async fn help(manager: &CommandManager, mut args: ArgumentManager) -> Result<(), CommandError> {
    if args.has_argument("command") {
        let arg_value = args.get_value("command")?.to_string_value()?;
        let commands = manager.get_commands().lock()?;
        let cmd = commands.iter().find(|command| *command.get_name() == *arg_value).ok_or(CommandError::CommandNotFound)?;
        manager.message(&format!("Usage: {}", cmd.get_usage()));
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
    let level = LogLevel::from_str(&arg_value).map_err(|e| CommandError::InvalidArgument(e.to_owned()))?;
    log::set_max_level(level.into());
    manager.message(format!("Log level set to {}", level));

    Ok(())
}