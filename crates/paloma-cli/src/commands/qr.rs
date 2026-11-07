//! QR image downloads use the signed CDN URL without forwarding Graph credentials.

use super::listing::{Pagination, Rows};
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
        #[command(flatten)]
        message: MessageArgs,
        #[command(flatten)]
        image: ImageArgs,
    },
    /// List message QR codes.
    List {
        #[command(flatten)]
        page: Pagination,
    },
    /// Show a message QR code and optionally save its image.
    Get {
        /// QR code id, as `qr list` shows it.
        code: String,
        #[command(flatten)]
        image: ImageArgs,
    },
    /// Change a QR code's prefilled message.
    Update {
        /// QR code id, as `qr list` shows it.
        code: String,
        #[command(flatten)]
        message: MessageArgs,
    },
    /// Delete a message QR code and expire its short link.
    Delete {
        /// QR code id, as `qr list` shows it.
        code: String,
    },
}

#[derive(Debug, Args)]
pub struct MessageArgs {
    /// The message the QR code fills in for the customer.
    #[arg(value_name = "MESSAGE")]
    text: String,
}

impl MessageArgs {
    fn value(&self) -> &str {
        &self.text
    }
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
    /// Generate an image; defaults to CODE.svg or CODE.png unless --out is set.
    #[arg(long, ignore_case = true)]
    image: Option<ImageFormat>,
    /// New destination file; existing files are refused.
    #[arg(long, short = 'O', requires = "image")]
    out: Option<PathBuf>,
}
impl ImageArgs {
    fn validate(&self, code: Option<&str>) -> Result<()> {
        let default = code
            .zip(self.image)
            .map(|(code, format)| image_path(code, format));
        if let Some(path) = self.out.as_ref().or(default.as_ref()) {
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
                validate::qr_message(message.value())?;
                image.validate(None)?;
            }
            Self::Get { code, image } => {
                validate::qr_code(code)?;
                image.validate(Some(code))?;
            }
            Self::Update { code, message } => {
                validate::qr_code(code)?;
                validate::qr_message(message.value())?;
            }
            Self::Delete { code } => validate::qr_code(code)?,
            Self::List { page } => page.check(Some(limits::MAX_QR_PAGE))?,
        }
        let phone = app.phone()?;
        match self {
            Self::Create { message, image } => {
                emit(
                    app,
                    qr::create(phone, message.value(), image.image.map(ImageFormat::api)),
                    image,
                    true,
                )
                .await
            }
            Self::Get { code, image } => {
                emit(
                    app,
                    qr::get(phone, &code, image.image.map(ImageFormat::api)),
                    image,
                    false,
                )
                .await
            }
            Self::Update { code, message } => {
                app.send_and_emit(&qr::update(phone, &code, message.value()), qr::code)
                    .await
            }
            Self::Delete { code } => {
                if !app
                    .confirm_or_abort(&format!("delete QR code {code} and expire its short link"))?
                {
                    return Ok(());
                }
                app.send_and_emit(&qr::delete(phone, &code), crate::app::fixed_id(&code))
                    .await
            }
            Self::List { page } => {
                super::listing::list(
                    app,
                    &page,
                    |after| qr::list(phone, page.limit, after),
                    Rows {
                        columns: COLUMNS,
                        id: "code",
                    },
                )
                .await
            }
        }
    }
}

fn image_path(code: &str, format: ImageFormat) -> PathBuf {
    PathBuf::from(format!("{code}.{}", format.api().to_ascii_lowercase()))
}

async fn emit(app: &App, request: Request, image: ImageArgs, created: bool) -> Result<()> {
    if app.dry_run() {
        let mut requests = vec![request];
        if image.image.is_some() {
            requests.push(Request::get_placeholder(
                "<qr_image_url from the QR response>",
            ));
        }
        return app.print_dry_run(&requests);
    }
    let response = app.fetch(&request).await?;
    let output = Output::response(&response)?;
    let code = qr::code(&output.data);
    // Everything after the create sits in this block, so any failure can say it exists.
    let result: Result<()> = async {
        let path = match (image.out, image.image) {
            (Some(path), _) => path,
            (None, Some(format)) => {
                let code = code
                    .as_deref()
                    .ok_or_else(|| Error::other("QR response has no code"))?;
                validate::qr_code(code)?;
                image_path(code, format)
            }
            (None, None) => return Ok(()),
        };
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
    app.out.emit(&output.id(code).table("/data", COLUMNS));
    Ok(())
}
