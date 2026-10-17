//! Migration is deliberately split: cloning assets must not silently switch billing.

use clap::Subcommand;
use paloma_core::{Error, Result, validate};
use paloma_graph::api::account;

use crate::app::{App, field_id};

#[derive(Debug, Subcommand)]
pub enum BillingCurrency {
    /// Clone the active Messaging account into a target billing currency.
    Start {
        /// Three-letter uppercase currency code, such as INR or BRL.
        #[arg(long)]
        currency: String,
        /// Optional Line of Credit id. Credit cards must be attached in Business Manager.
        #[arg(long)]
        extended_credit_id: Option<String>,
    },
    /// Show migration progress and the destination account.
    Status { migration_id: String },
    /// Switch billing to the destination account; the old account stops sending.
    Complete { migration_id: String },
}

impl BillingCurrency {
    pub async fn run(self, app: &App) -> Result<()> {
        match self {
            Self::Start {
                currency,
                extended_credit_id,
            } => {
                validate::currency_code(&currency)?;
                if let Some(id) = &extended_credit_id {
                    validate::graph_node_id(id, "extended credit id")?;
                }
                let waba = app.ctx()?.require_waba_id()?.to_owned();
                if !app
                    .confirm_or_abort(&format!("clone Messaging account {waba} into {currency}"))?
                {
                    return Ok(());
                }
                app.send_and_emit(
                    &account::start_currency_migration(
                        &waba,
                        &currency,
                        extended_credit_id.as_deref(),
                    ),
                    |v| {
                        v.get("migration_id")
                            .and_then(|id| id.as_str())
                            .map(str::to_owned)
                    },
                )
                .await
            }
            Self::Status { migration_id } => {
                validate::graph_node_id(&migration_id, "migration id")?;
                app.send_and_emit(&account::currency_migration_status(&migration_id), field_id)
                    .await
            }
            Self::Complete { migration_id } => {
                validate::graph_node_id(&migration_id, "migration id")?;
                if !app.confirm_or_abort(&format!("complete migration {migration_id}: switch billing and stop the old account from sending"))? {
                    return Ok(());
                }
                let request = account::complete_currency_migration(&migration_id);
                if app.dry_run() {
                    return app.print_dry_run(&[
                        account::currency_migration_status(&migration_id),
                        request,
                    ]);
                }
                {
                    let current = app
                        .fetch(&account::currency_migration_status(&migration_id))
                        .await?
                        .json()?;
                    if !matches!(
                        current.get("status").and_then(|v| v.as_str()),
                        Some("READY_TO_COMPLETE" | "FAILED_TO_COMPLETE")
                    ) {
                        return Err(Error::other(
                            "migration is not ready to complete; run account billing-currency status and verify the destination assets and payment method",
                        ));
                    }
                }
                app.send_and_emit(&request, field_id).await
            }
        }
    }
}
