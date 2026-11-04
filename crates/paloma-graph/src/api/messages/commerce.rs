//! Catalog messages: one product, a sectioned product list, or the catalog button.

#[allow(clippy::wildcard_imports)] // part of `messages`: shares the envelope and helpers
use super::*;

/// One catalog product. Meta doesn't allow a header on this kind, and the body is optional.
pub fn interactive_product(
    to: &str,
    catalog_id: &str,
    product: &str,
    body: Option<&str>,
    footer: Option<&str>,
    reply_to: Option<&str>,
) -> Value {
    let text = InteractiveText {
        header: None,
        body: body.unwrap_or_default().to_owned(),
        footer: footer.map(str::to_owned),
    };
    interactive(
        to,
        "product",
        &text,
        json!({ "catalog_id": catalog_id, "product_retailer_id": product }),
        reply_to,
    )
}

/// A titled section of a multi-product message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductSection {
    pub title: String,
    pub products: Vec<String>,
}

pub fn interactive_product_list(
    to: &str,
    text: &InteractiveText,
    catalog_id: &str,
    sections: &[ProductSection],
    reply_to: Option<&str>,
) -> Value {
    let sections: Vec<Value> = sections
        .iter()
        .map(|section| {
            let items: Vec<Value> = section
                .products
                .iter()
                .map(|product| json!({ "product_retailer_id": product }))
                .collect();
            json!({ "title": section.title, "product_items": items })
        })
        .collect();
    interactive(
        to,
        "product_list",
        text,
        json!({ "catalog_id": catalog_id, "sections": sections }),
        reply_to,
    )
}

/// `thumbnail` is the product whose image heads the message. Without it, `parameters` is
/// left out and Meta uses the first item in the catalog.
pub fn interactive_catalog(
    to: &str,
    body: &str,
    footer: Option<&str>,
    thumbnail: Option<&str>,
    reply_to: Option<&str>,
) -> Value {
    let text = InteractiveText {
        header: None,
        body: body.to_owned(),
        footer: footer.map(str::to_owned),
    };
    let mut action = json!({ "name": "catalog_message" });
    if let Some(thumbnail) = thumbnail {
        action["parameters"] = json!({ "thumbnail_product_retailer_id": thumbnail });
    }
    interactive(to, "catalog_message", &text, action, reply_to)
}
