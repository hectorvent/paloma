//! QR image downloads use the signed CDN URL without forwarding Graph credentials.

use crate::app::App;
use crate::output::Output;
use clap::{Args, Subcommand, ValueEnum};
use paloma_core::{Error, Result, limits, validate};
use paloma_graph::{Request, api::qr};
use std::path::PathBuf;

const COLUMNS: &[&str] = &["code", "prefilled_message", "deep_link_url"];

#[derive(Debug, Subcommand)]
pub enum QrCommand {
    /// Create a message QR code and optional image file.
    Create {
        #[arg(long)]
        message: String,
        #[command(flatten)]
        image: ImageArgs,
    },
    /// List message QR codes.
    List {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        after: Option<String>,
        /// Page size, at most 25.
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Show a message QR code and optionally save its image.
    Get {
        code: String,
        #[command(flatten)]
        image: ImageArgs,
    },
    /// Change a QR code's prefilled message.
    Update {
        code: String,
        #[arg(long)]
        message: String,
    },
    /// Delete a message QR code and expire its short link.
    Delete { code: String },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ImageFormat {
    Svg,
    Png,
}
impl ImageFormat {
    fn api(self) -> &'static str {
        match self {
            Self::Svg => "SVG",
            Self::Png => "PNG",
        }
    }
}

#[derive(Debug, Args)]
pub struct ImageArgs {
    /// Generate an image in this format and save it with --out.
    #[arg(long, requires = "out")]
    image: Option<ImageFormat>,
    /// New destination file; existing files are refused.
    #[arg(long, requires = "image")]
    out: Option<PathBuf>,
}
impl ImageArgs {
    fn validate(&self) -> Result<()> {
        if let Some(path) = &self.out {
            if path.as_os_str().is_empty() || path.symlink_metadata().is_ok() {
                return Err(Error::validation("--out must name a new file"));
            }
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));
            if !parent.is_dir() {
                return Err(Error::validation("--out parent directory must exist"));
            }
        }
        Ok(())
    }
}

impl QrCommand {
    pub async fn run(self, app: &App) -> Result<()> {
        match &self {
            Self::Create { message, image } => {
                validate::max_chars(message, limits::MAX_QR_MESSAGE, "QR message")?;
                image.validate()?;
            }
            Self::Get { code, image } => {
                validate::qr_code(code)?;
                image.validate()?;
            }
            Self::Update { code, message } => {
                validate::qr_code(code)?;
                validate::max_chars(message, limits::MAX_QR_MESSAGE, "QR message")?;
            }
            Self::Delete { code } => validate::qr_code(code)?,
            Self::List { limit, .. } => validate::page_size(*limit, Some(limits::MAX_QR_PAGE))?,
        }
        let phone = app.ctx()?.require_phone_number_id()?.to_owned();
        match self {
            Self::Create { message, image } => {
                emit(
                    app,
                    qr::create(&phone, &message, image.image.map(ImageFormat::api)),
                    image,
                    true,
                )
                .await
            }
            Self::Get { code, image } => {
                emit(
                    app,
                    qr::get(&phone, &code, image.image.map(ImageFormat::api)),
                    image,
                    false,
                )
                .await
            }
            Self::Update { code, message } => {
                app.send_and_emit(&qr::update(&phone, &code, &message), qr::code)
                    .await
            }
            Self::Delete { code } => {
                if !app
                    .confirm_or_abort(&format!("delete QR code {code} and expire its short link"))?
                {
                    return Ok(());
                }
                app.send_and_emit(&qr::delete(&phone, &code), |_| Some(code.clone()))
                    .await
            }
            Self::List { all, after, limit } => {
                super::template::list(
                    app,
                    all,
                    after,
                    |after| qr::list(&phone, limit, after),
                    COLUMNS,
                )
                .await
            }
        }
    }
}

async fn emit(app: &App, request: Request, image: ImageArgs, created: bool) -> Result<()> {
    if app.dry_run() {
        let mut requests = vec![request];
        if image.out.is_some() {
            requests
                .push(Request::get_url("<qr_image_url from the QR response>").unauthenticated());
        }
        return app.print_dry_run(&requests);
    }
    let response = app.fetch(&request).await?;
    let output = Output::response(&response)?;
    let code = qr::code(&output.data);
    if let Some(path) = image.out {
        let result: Result<()> = async {
            let url = qr::image_url(&output.data)
                .ok_or_else(|| Error::other("QR response has no image URL"))?;
            let bytes = app
                .client()?
                .download_new(&Request::get_url(url).unauthenticated(), &path)
                .await?;
            app.out
                .note(format!("wrote {bytes} bytes to {}", path.display()));
            Ok(())
        }
        .await;
        if result.is_err() && created {
            app.out.warn(format!(
                "QR code {} was created, but saving its image failed; use qr get --image to retry",
                code.as_deref().unwrap_or("(code missing from response)")
            ));
        }
        result?;
    }
    app.out.emit(&output.id(code).table("/data", COLUMNS));
    Ok(())
}
