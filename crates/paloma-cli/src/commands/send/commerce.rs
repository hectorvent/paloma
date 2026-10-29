//! Catalog messages: one product, a sectioned product list, or the catalog button.

#[allow(clippy::wildcard_imports)] // part of `send`: shares its imports and helpers
use super::*;

#[derive(Debug, Args)]
pub struct ProductMessage {
    /// Recipient (E.164 number or business-scoped user ID).
    pub(super) to: String,
    /// The catalog holding the product.
    #[arg(long)]
    pub(super) catalog_id: String,
    /// The product's retailer id (its SKU; Content ID in Commerce Manager).
    #[arg(long, value_name = "SKU")]
    pub(super) product: String,
    /// Body text.
    #[arg(long)]
    pub(super) body: Option<String>,
    /// Footer text.
    #[arg(long)]
    pub(super) footer: Option<String>,
    #[command(flatten)]
    pub(super) reply: Reply,
    #[command(flatten)]
    pub(super) policy: Policy,
}

#[derive(Debug, Args)]
pub struct ProductList {
    /// Recipient (E.164 number or business-scoped user ID).
    pub(super) to: String,
    /// The catalog holding the products.
    #[arg(long)]
    pub(super) catalog_id: String,
    /// Header text.
    #[arg(long)]
    pub(super) header: String,
    /// Body text.
    #[arg(long)]
    pub(super) body: String,
    /// Footer text.
    #[arg(long)]
    pub(super) footer: Option<String>,
    /// A section as `Title:sku1,sku2`; repeatable. Write a comma in a SKU as `\,`.
    #[arg(long = "section", value_name = "SECTION", required = true)]
    pub(super) sections: Vec<String>,
    #[command(flatten)]
    pub(super) reply: Reply,
    #[command(flatten)]
    pub(super) policy: Policy,
}

#[derive(Debug, Args)]
pub struct Catalog {
    /// Recipient (E.164 number or business-scoped user ID).
    pub(super) to: String,
    /// Body text.
    #[arg(long)]
    pub(super) body: String,
    /// Footer text.
    #[arg(long)]
    pub(super) footer: Option<String>,
    /// SKU of the product whose image heads the message. Meta defaults to the first item
    /// in the catalog.
    #[arg(long, value_name = "SKU")]
    pub(super) thumbnail: Option<String>,
    #[command(flatten)]
    pub(super) reply: Reply,
    #[command(flatten)]
    pub(super) policy: Policy,
}

pub(super) async fn send_product(app: &App, a: ProductMessage) -> Result<()> {
    validate::recipient(&a.to)?;
    validate::not_blank(&a.catalog_id, "--catalog-id")?;
    validate::not_blank(&a.product, "--product")?;
    if let Some(body) = &a.body {
        check_body(body)?;
    }
    check_footer(a.footer.as_deref())?;
    let reply_to = a.reply.id()?;
    a.policy.enforce(&a.to)?;
    let payload = messages::interactive_product(
        &a.to,
        &a.catalog_id,
        &a.product,
        a.body.as_deref(),
        a.footer.as_deref(),
        reply_to,
    );
    dispatch(app, payload).await
}

pub(super) async fn send_product_list(app: &App, a: ProductList) -> Result<()> {
    validate::recipient(&a.to)?;
    validate::not_blank(&a.catalog_id, "--catalog-id")?;
    validate::not_blank(&a.header, "--header")?;
    validate::max_chars(&a.header, limits::MAX_INTERACTIVE_HEADER, "--header")?;
    check_body(&a.body)?;
    check_footer(a.footer.as_deref())?;
    let sections = parse_product_sections(&a.sections)?;
    let reply_to = a.reply.id()?;
    a.policy.enforce(&a.to)?;
    let text = InteractiveText {
        header: Some(InteractiveHeader::Text(a.header.clone())),
        body: a.body.clone(),
        footer: a.footer.clone(),
    };
    let payload =
        messages::interactive_product_list(&a.to, &text, &a.catalog_id, &sections, reply_to);
    dispatch(app, payload).await
}

pub(super) async fn send_catalog(app: &App, a: Catalog) -> Result<()> {
    validate::recipient(&a.to)?;
    check_body(&a.body)?;
    check_footer(a.footer.as_deref())?;
    if let Some(thumbnail) = &a.thumbnail {
        validate::not_blank(thumbnail, "--thumbnail")?;
    }
    let reply_to = a.reply.id()?;
    a.policy.enforce(&a.to)?;
    let payload = messages::interactive_catalog(
        &a.to,
        &a.body,
        a.footer.as_deref(),
        a.thumbnail.as_deref(),
        reply_to,
    );
    dispatch(app, payload).await
}

/// `Title:sku1,sku2`. Meta counts products across every section.
pub(super) fn parse_product_sections(specs: &[String]) -> Result<Vec<messages::ProductSection>> {
    if specs.len() > limits::MAX_PRODUCT_LIST_SECTIONS {
        return Err(Error::validation(format!(
            "a product list holds at most {} sections (got {})",
            limits::MAX_PRODUCT_LIST_SECTIONS,
            specs.len()
        )));
    }
    let sections = specs
        .iter()
        .map(|spec| {
            let (title, products) = spec.split_once(':').ok_or_else(|| {
                Error::validation(format!("--section '{spec}' must look like Title:sku1,sku2"))
            })?;
            validate::not_blank(title, "section title")?;
            validate::max_chars(
                title.trim(),
                limits::MAX_PRODUCT_LIST_SECTION_TITLE,
                "section title",
            )?;
            let products: Vec<String> = split_rows(products)
                .iter()
                .map(|product| product.trim().to_owned())
                .filter(|product| !product.is_empty())
                .collect();
            if products.is_empty() {
                return Err(Error::validation(format!(
                    "--section '{title}' has no products"
                )));
            }
            Ok(messages::ProductSection {
                title: title.trim().to_owned(),
                products,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let total: usize = sections.iter().map(|section| section.products.len()).sum();
    if total > limits::MAX_PRODUCT_LIST_PRODUCTS {
        return Err(Error::validation(format!(
            "a product list holds at most {} products in total (got {total})",
            limits::MAX_PRODUCT_LIST_PRODUCTS
        )));
    }
    Ok(sections)
}
