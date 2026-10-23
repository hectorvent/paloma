//! Per-invocation wiring, built once in `main` and borrowed by every command.
//!
//! Everything expensive or credential-bearing is lazy: the config file is read, the
//! context resolved and the HTTP client built only when a command first asks, so
//! offline commands (`link create`, `config path`, `--help`) touch none of it.

use std::io::IsTerminal;
use std::sync::{Arc, OnceLock};

use paloma_config::{Config, ConfigFile, Context, KeyringStore, Overrides, SecretStore};
use paloma_core::{Error, Result};
use paloma_graph::{Client, ClientConfig, Request, Response, RetryNotice};

use crate::cli::GlobalArgs;
use crate::output::{Output, Printer};

#[derive(Debug)]
pub struct App {
    pub globals: GlobalArgs,
    pub out: Printer,
    config_file: OnceLock<ConfigFile>,
    ctx: OnceLock<Arc<Context>>,
    client: OnceLock<Client>,
    store: Arc<dyn SecretStore>,
}

impl App {
    pub fn new(globals: GlobalArgs) -> Self {
        let out = Printer::new(globals.output, globals.quiet, globals.no_color);
        Self {
            globals,
            out,
            config_file: OnceLock::new(),
            ctx: OnceLock::new(),
            client: OnceLock::new(),
            store: Arc::new(KeyringStore),
        }
    }

    pub fn dry_run(&self) -> bool {
        self.globals.dry_run
    }

    pub fn store(&self) -> &dyn SecretStore {
        self.store.as_ref()
    }

    pub fn config_file(&self) -> Result<&ConfigFile> {
        if let Some(f) = self.config_file.get() {
            return Ok(f);
        }
        let (file, note) = ConfigFile::open(self.globals.config.as_deref())?;
        if let Some(n) = note {
            self.out.note(n);
        }
        Ok(self.config_file.get_or_init(|| file))
    }

    pub fn config(&self) -> Result<Config> {
        self.config_file()?.load()
    }

    /// The selected profile name (flag/env, else the config default).
    pub fn profile_name(&self) -> Result<String> {
        match self
            .globals
            .profile
            .as_deref()
            .filter(|p| !p.trim().is_empty())
        {
            Some(p) => Ok(p.to_owned()),
            None => Ok(self.config()?.default_profile_name().to_owned()),
        }
    }

    pub fn overrides(&self) -> Overrides {
        let g = &self.globals;
        Overrides {
            profile: g.profile.clone(),
            token: g.token.clone(),
            phone_number_id: g.phone_number_id.clone(),
            waba_id: g.waba_id.clone(),
            app_id: g.app_id.clone(),
            app_secret: g.app_secret.clone(),
            api_version: g.api_version.clone(),
            messaging_account_id: g.messaging_account_id.clone(),
        }
    }

    /// The resolved context. Resolution warnings are printed once, here.
    pub fn ctx(&self) -> Result<&Arc<Context>> {
        if let Some(c) = self.ctx.get() {
            return Ok(c);
        }
        let config = self.config()?;
        let ctx = Context::resolve(
            &config,
            &self.overrides(),
            paloma_graph::DEFAULT_API_VERSION,
            Arc::clone(&self.store),
        );
        for w in &ctx.warnings {
            self.out.warn(w);
        }
        Ok(self.ctx.get_or_init(|| Arc::new(ctx)))
    }

    pub fn client(&self) -> Result<&Client> {
        if let Some(c) = self.client.get() {
            return Ok(c);
        }
        let ctx = self.ctx()?;
        let mut config = ClientConfig::new(&self.globals.graph_url, &ctx.api_version);
        config.timeout = self.globals.timeout;
        let out = self.out.clone();
        config.on_retry = Some(RetryNotice::new(move |message| out.note(message)));
        let client = Client::new(config, Arc::clone(ctx) as Arc<dyn paloma_core::Credentials>)?;
        Ok(self.client.get_or_init(|| client))
    }

    /// Execute a request regardless of `--dry-run`: for reads a command needs in order to
    /// build its real request (lookups, planning).
    pub async fn fetch(&self, req: &Request) -> Result<Response> {
        self.client()?.execute(req).await
    }

    /// Execute a request, or under `--dry-run` print it and return `None`.
    pub async fn send(&self, req: &Request) -> Result<Option<Response>> {
        if self.dry_run() {
            self.print_dry_run(std::slice::from_ref(req))?;
            return Ok(None);
        }
        self.fetch(req).await.map(Some)
    }

    /// Print what would be sent. Several requests render as a JSON array.
    pub fn print_dry_run(&self, reqs: &[Request]) -> Result<()> {
        let client = self.client()?;
        self.out.note("dry-run: no request sent");
        let described: Vec<_> = reqs.iter().map(|r| client.describe(r)).collect();
        let (data, raw) = match reqs {
            [one] => (
                described.into_iter().next().unwrap_or_default(),
                one.json_body()
                    .map(|body| one.diagnostic_json(body).to_string()),
            ),
            _ => (serde_json::Value::Array(described), None),
        };
        // `-o raw` shows just the body that would be sent; json/yaml/table the whole request.
        let raw = matches!(self.out.format(), crate::cli::Format::Raw)
            .then_some(raw)
            .flatten();
        self.out.emit(&Output {
            raw,
            ..Output::new(data)
        });
        Ok(())
    }

    /// Send and print the response; the common shape of a single-call command.
    pub async fn send_and_emit(
        &self,
        req: &Request,
        primary_id: impl Fn(&serde_json::Value) -> Option<String>,
    ) -> Result<()> {
        if let Some(resp) = self.send(req).await? {
            let out = Output::response(&resp)?;
            let id = primary_id(&out.data);
            self.out.emit(&out.id(id));
        }
        Ok(())
    }

    /// Confirm a destructive action: `true` under `--yes`; refuses without a terminal.
    pub fn confirm(&self, what: &str) -> Result<bool> {
        self.ensure_can_confirm(what)?;
        if self.globals.yes || self.dry_run() {
            return Ok(true);
        }
        dialoguer::Confirm::new()
            .with_prompt(format!("{what}?"))
            .default(false)
            .interact()
            .map_err(|e| Error::other(format!("prompt failed: {e}")))
    }

    /// Fail if a confirmation could never pass, for commands that would otherwise spend a
    /// call before reaching [`Self::confirm`].
    pub fn ensure_can_confirm(&self, what: &str) -> Result<()> {
        if self.globals.yes
            || self.dry_run()
            || (std::io::stdin().is_terminal() && std::io::stderr().is_terminal())
        {
            return Ok(());
        }
        Err(Error::validation(format!(
            "refusing to {what} without confirmation; pass --yes"
        )))
    }

    /// Confirm, or stop with exit 0 and an "aborted" note.
    pub fn confirm_or_abort(&self, what: &str) -> Result<bool> {
        let ok = self.confirm(what)?;
        if !ok {
            self.out.info("aborted");
        }
        Ok(ok)
    }
}

/// The `id` field of a response, for `--quiet`.
pub fn field_id(v: &serde_json::Value) -> Option<String> {
    match v.get("id")? {
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// No primary id.
pub fn no_id(_: &serde_json::Value) -> Option<String> {
    None
}
