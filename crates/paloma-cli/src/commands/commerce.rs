//! Cart and catalog settings are query parameters, including explicit false values.
use super::util::toggle;
use crate::app::{App, no_id};
use crate::output::Output;
use clap::{Args, Subcommand};
use paloma_core::{Error, Result};
use paloma_graph::api::commerce;

#[derive(Debug, Subcommand)]
pub enum CommerceCommand {
    /// Read the current shopping cart and catalog visibility settings.
    Get,
    /// Update cart or catalog visibility; disabling confirms before writing.
    Set(SetArgs),
}

#[allow(clippy::struct_excessive_bools)] // --x/--no-x flag pairs, as the CLI grammar asks
#[derive(Debug, Args)]
pub struct SetArgs {
    /// Turn the shopping cart on.
    #[arg(long, overrides_with = "no_cart")]
    cart: bool,
    /// Turn the shopping cart off.
    #[arg(long, overrides_with = "cart")]
    no_cart: bool,
    /// Show the catalog on the business profile and in chats.
    #[arg(long, overrides_with = "no_catalog")]
    catalog: bool,
    /// Hide the catalog.
    #[arg(long, overrides_with = "catalog")]
    no_catalog: bool,
}

impl SetArgs {
    fn cart_enabled(&self) -> Option<bool> {
        toggle(self.cart, self.no_cart)
    }

    fn catalog_visible(&self) -> Option<bool> {
        toggle(self.catalog, self.no_catalog)
    }
}

impl CommerceCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        if matches!(&self, Self::Set(a) if a.cart_enabled().is_none() && a.catalog_visible().is_none())
        {
            return Err(Error::validation(
                "pass --cart, --no-cart, --catalog or --no-catalog",
            ));
        }
        let phone = app.ctx()?.require_phone_number_id()?.to_owned();
        match self {
            Self::Get => {
                if let Some(resp) = app.send(&commerce::get(&phone)).await? {
                    let id = resp
                        .json()?
                        .pointer("/data/0/id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned);
                    app.out.emit(
                        &Output::response(&resp)?
                            .id(id)
                            .table("/data", &["id", "is_cart_enabled", "is_catalog_visible"]),
                    );
                }
                Ok(())
            }
            Self::Set(a) => {
                if (a.cart_enabled() == Some(false) || a.catalog_visible() == Some(false))
                    && !app.confirm_or_abort("disable shopping cart or catalog visibility")?
                {
                    return Ok(());
                }
                app.send_and_emit(
                    &commerce::set(&phone, a.cart_enabled(), a.catalog_visible()),
                    no_id,
                )
                .await
            }
        }
    }
}
