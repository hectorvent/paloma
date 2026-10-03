use clap::Subcommand;
use paloma_core::Result;
use paloma_graph::api::block;

use crate::app::{App, no_id};
use crate::output::Output;

#[derive(Debug, Subcommand)]
pub enum BlockCommand {
    /// Block one or more users.
    Add {
        #[arg(required = true, value_name = "USER")]
        users: Vec<String>,
    },
    /// Unblock one or more users.
    Remove {
        #[arg(required = true, value_name = "USER")]
        users: Vec<String>,
    },
    /// List blocked users.
    List,
}

impl BlockCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        let phone = app.ctx()?.require_phone_number_id()?.to_owned();
        match self {
            Self::Add { users } => app.send_and_emit(&block::add(&phone, &users), no_id).await,
            Self::Remove { users } => {
                app.send_and_emit(&block::remove(&phone, &users), no_id)
                    .await
            }
            Self::List => {
                if let Some(resp) = app.send(&block::list(&phone)).await? {
                    app.out
                        .emit(&Output::response(&resp)?.table("/data", &["wa_id", "user"]));
                }
                Ok(())
            }
        }
    }
}
