//! Conversational settings share the manifest endpoint, with explicit empty lists for
//! clearing prompts and commands rather than treating omitted flags as deletion.

use super::util::{read_json, toggle};
use crate::app::{App, field_id, no_id};
use clap::{Args, Subcommand};
use paloma_core::{Error, Result, limits, validate};
use paloma_graph::api::conversational_automation::{self as automation, Command, Settings};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum AutomationCommand {
    /// Show the WABA's bots.
    #[command(subcommand)]
    Bot(BotCommand),
    /// Show welcome-message, ice-breaker and command settings.
    Get,
    /// Update conversational components on the active phone number.
    Set(SetArgs),
}

#[derive(Debug, Subcommand)]
pub enum BotCommand {
    /// Show a WABA bot by its id.
    Get {
        /// Bot id.
        bot_id: String,
    },
}

#[allow(clippy::struct_excessive_bools)] // --x/--no-x flag pairs, as the CLI grammar asks
#[derive(Debug, Args)]
pub struct SetArgs {
    /// Send the welcome message when a user opens a chat for the first time.
    #[arg(long, overrides_with = "no_welcome")]
    welcome: bool,
    /// Stop sending the welcome message.
    #[arg(long, overrides_with = "welcome")]
    no_welcome: bool,
    /// Ice breaker; repeat for up to four prompts.
    #[arg(long, visible_alias = "ice-breaker")]
    prompt: Vec<String>,
    /// Command without leading slash, followed by its hint; repeatable.
    #[arg(long, value_name = "NAME:DESCRIPTION")]
    command: Vec<String>,
    /// Remove every ice breaker.
    #[arg(long, conflicts_with = "prompt")]
    clear_prompts: bool,
    /// Remove every bot command.
    #[arg(long, conflicts_with = "command")]
    clear_commands: bool,
    /// Full conversational-automation JSON object.
    #[arg(short = 'f', long, conflicts_with_all = ["welcome", "no_welcome", "prompt", "command", "clear_prompts", "clear_commands"])]
    file: Option<PathBuf>,
}

impl SetArgs {
    fn body(self) -> Result<Value> {
        let body = if let Some(file) = self.file {
            read_json(&file)?
        } else {
            let commands: Vec<Command> = self
                .command
                .iter()
                .map(|value| {
                    let (name, description) = value.split_once([':', '=']).ok_or_else(|| {
                        Error::validation("--command must be NAME:DESCRIPTION or NAME=DESCRIPTION")
                    })?;
                    Ok(Command {
                        command_name: name.to_owned(),
                        command_description: description.to_owned(),
                    })
                })
                .collect::<Result<_>>()?;
            Settings {
                enable_welcome_message: toggle(self.welcome, self.no_welcome),
                prompts: (self.clear_prompts || !self.prompt.is_empty()).then_some(self.prompt),
                commands: (self.clear_commands || !commands.is_empty()).then_some(commands),
            }
            .body()
        };
        validate_body(&body)?;
        Ok(body)
    }
}

fn validate_body(body: &Value) -> Result<()> {
    if body.as_object().is_none_or(serde_json::Map::is_empty) {
        return Err(Error::validation(
            "automation settings must be a nonempty JSON object; pass settings or --file",
        ));
    }
    if body
        .get("enable_welcome_message")
        .is_some_and(|v| !v.is_boolean())
    {
        return Err(Error::validation("enable_welcome_message must be boolean"));
    }
    if let Some(prompts) = body.get("prompts") {
        let prompts = prompts
            .as_array()
            .ok_or_else(|| Error::validation("prompts must be an array"))?;
        if prompts.len() > limits::MAX_AUTOMATION_PROMPTS {
            return Err(Error::validation(format!(
                "at most {} prompts are allowed",
                limits::MAX_AUTOMATION_PROMPTS
            )));
        }
        for prompt in prompts {
            let text = prompt
                .as_str()
                .ok_or_else(|| Error::validation("each prompt must be a string"))?;
            validate::not_blank(text, "prompt")?;
            validate::max_chars(text, limits::MAX_AUTOMATION_PROMPT, "prompt")?;
        }
    }
    if let Some(commands) = body.get("commands") {
        let commands = commands
            .as_array()
            .ok_or_else(|| Error::validation("commands must be an array"))?;
        if commands.len() > limits::MAX_AUTOMATION_COMMANDS {
            return Err(Error::validation(format!(
                "at most {} commands are allowed",
                limits::MAX_AUTOMATION_COMMANDS
            )));
        }
        let mut names = std::collections::HashSet::new();
        for command in commands {
            let name = command
                .get("command_name")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::validation("each command needs a command_name string"))?;
            let hint = command
                .get("command_description")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Error::validation("each command needs a command_description string")
                })?;
            validate::not_blank(name, "command name")?;
            validate::not_blank(hint, "command description")?;
            if !names.insert(name) {
                return Err(Error::validation(format!(
                    "duplicate command name '{name}'"
                )));
            }
            if name.starts_with('/') {
                return Err(Error::validation(
                    "command names must omit the leading slash",
                ));
            }
            validate::max_chars(name, limits::MAX_AUTOMATION_COMMAND_NAME, "command name")?;
            validate::max_chars(
                hint,
                limits::MAX_AUTOMATION_COMMAND_DESCRIPTION,
                "command description",
            )?;
        }
    }
    Ok(())
}

impl AutomationCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Bot(BotCommand::Get { bot_id }) => {
                validate::numeric_id(&bot_id, "bot id")?;
                app.send_and_emit(&automation::bot(&bot_id), field_id).await
            }
            Self::Get => {
                let phone = app.phone()?;
                app.send_and_emit(&automation::get(phone), field_id).await
            }
            Self::Set(args) => {
                let body = args.body()?;
                let phone = app.phone()?;
                app.send_and_emit(&automation::set(phone, body), no_id)
                    .await
            }
        }
    }
}
