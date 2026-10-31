//! Meta's template library: ready-made templates that skip review once created.

#[allow(clippy::wildcard_imports)] // part of `template`: shares its imports and helpers
use super::*;

#[derive(Debug, Subcommand)]
pub enum LibraryCommand {
    /// List library templates.
    List(LibraryList),
    /// Create a template on this WABA from a library template.
    Create(LibraryCreate),
}

#[derive(Debug, Args)]
pub struct LibraryList {
    /// Find this text in a template's name, content, header, body or footer.
    #[arg(long)]
    pub(super) search: Option<String>,
    /// Filter by topic, e.g. ORDER_MANAGEMENT.
    #[arg(long)]
    pub(super) topic: Option<String>,
    /// Filter by use case, e.g. SHIPMENT_CONFIRMATION.
    #[arg(long)]
    pub(super) usecase: Option<String>,
    /// Filter by industry, e.g. E_COMMERCE.
    #[arg(long)]
    pub(super) industry: Option<String>,
    /// Filter by language code.
    #[arg(long)]
    pub(super) language: Option<String>,
    /// Filter by library template name.
    #[arg(long)]
    pub(super) name: Option<String>,
    #[command(flatten)]
    page: Pagination,
}

#[derive(Debug, Args)]
pub struct LibraryCreate {
    /// Name for the new template.
    #[arg(long)]
    pub(super) name: String,
    /// The exact name of the library template.
    #[arg(long)]
    pub(super) library_template: String,
    /// Language code, e.g. en_US.
    #[arg(long)]
    pub(super) language: String,
    /// JSON array for library_template_button_inputs. Meta requires it for a utility
    /// template with buttons.
    #[arg(long, value_name = "PATH")]
    pub(super) button_inputs_file: Option<PathBuf>,
    /// JSON object for library_template_body_inputs.
    #[arg(long, value_name = "PATH")]
    pub(super) body_inputs_file: Option<PathBuf>,
}

pub(super) async fn library(app: &App, command: LibraryCommand) -> Result<()> {
    match command {
        LibraryCommand::List(a) => {
            a.page.check(None)?;
            let filter = LibraryFilter {
                search: a.search,
                topic: a.topic,
                usecase: a.usecase,
                industry: a.industry,
                language: a.language,
                name: a.name,
            };
            list(
                app,
                &a.page,
                |after| templates::library_list(&filter, after).query_opt("limit", a.page.limit),
                Rows {
                    columns: LIBRARY_COLUMNS,
                    id: "id",
                },
            )
            .await
        }
        LibraryCommand::Create(a) => {
            validate::template_name(&a.name)?;
            validate::not_blank(&a.library_template, "--library-template")?;
            validate::not_blank(&a.language, "--language")?;
            let button_inputs = a.button_inputs_file.as_deref().map(read_json).transpose()?;
            if button_inputs.as_ref().is_some_and(|v| !v.is_array()) {
                return Err(Error::validation(
                    "--button-inputs-file must hold a JSON array",
                ));
            }
            let body_inputs = a.body_inputs_file.as_deref().map(read_json).transpose()?;
            if body_inputs.as_ref().is_some_and(|v| !v.is_object()) {
                return Err(Error::validation(
                    "--body-inputs-file must hold a JSON object",
                ));
            }
            let body = templates::library_template(&LibraryTemplate {
                name: a.name,
                language: a.language,
                library_template: a.library_template,
                button_inputs,
                body_inputs,
            });
            let waba = app.ctx()?.require_waba_id()?.to_owned();
            app.send_and_emit(&templates::create(&waba, body), field_id)
                .await
        }
    }
}
