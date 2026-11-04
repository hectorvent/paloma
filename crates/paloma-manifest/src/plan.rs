//! Turning a manifest file into plans: parse every document, expand its placeholders, and
//! decide which phone number and WABA each one targets. It lives here, not in the CLI, so
//! the rule for picking a document's target is written once and unit-tested.

use paloma_core::{Error, Result};

use crate::interpolate::{Scheme, interpolate};
use crate::{Kind, Manifest, yaml};

/// The ids a kind reads and writes under. Kinds see this instead of the CLI's whole
/// context, so the manifest crate doesn't depend on how the CLI resolves its settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::struct_field_names)] // the names Meta and the CLI use for these ids
pub struct Scope {
    waba_id: Option<String>,
    phone_number_id: Option<String>,
    app_id: Option<String>,
}

impl Scope {
    pub fn new(waba_id: Option<&str>, phone_number_id: Option<&str>, app_id: Option<&str>) -> Self {
        let own = |v: Option<&str>| v.filter(|s| !s.trim().is_empty()).map(str::to_owned);
        Self {
            waba_id: own(waba_id),
            phone_number_id: own(phone_number_id),
            app_id: own(app_id),
        }
    }

    pub fn waba_id(&self) -> Option<&str> {
        self.waba_id.as_deref()
    }

    pub fn phone_number_id(&self) -> Option<&str> {
        self.phone_number_id.as_deref()
    }

    pub fn app_id(&self) -> Option<&str> {
        self.app_id.as_deref()
    }

    pub fn require_phone_number_id(&self) -> Result<&str> {
        self.phone_number_id().ok_or_else(|| {
            Error::auth(
                "no phone number id: set metadata.phoneNumberId, your profile, or --phone-number-id",
            )
        })
    }

    pub fn require_waba_id(&self) -> Result<&str> {
        self.waba_id()
            .ok_or_else(|| Error::auth("no WABA id: set it in your profile or pass --waba-id"))
    }

    /// The scope one document runs in. A phone number id given for this run (`--phone-number-id`)
    /// wins over the document's `metadata.phoneNumberId`, which wins over the profile's.
    #[must_use]
    pub fn for_document(&self, manifest: &Manifest, flag_phone_number_id: Option<&str>) -> Self {
        let phone = flag_phone_number_id
            .map(str::to_owned)
            .or_else(|| manifest.phone_number_id())
            .or_else(|| self.phone_number_id.clone());
        Self {
            phone_number_id: phone.filter(|s| !s.trim().is_empty()),
            ..self.clone()
        }
    }

    /// `Kind/<id>`, naming what a document targets in `diff` and `apply` output.
    pub fn label(&self, kind: Kind) -> String {
        let id = if kind.phone_scoped() {
            self.phone_number_id()
        } else {
            self.waba_id()
        };
        format!("{}/{}", kind.name(), id.unwrap_or("?"))
    }
}

/// Every document in a manifest file, parsed and with `${...}` placeholders expanded.
/// All of them are checked before any is planned, so a bad document fails the whole run
/// before a single read.
pub fn load(
    text: &str,
    origin: &str,
    lookup: &impl Fn(Scheme, &str) -> Option<String>,
) -> Result<Vec<Manifest>> {
    yaml::read_documents(text, origin)?
        .iter()
        .map(|doc| Manifest::from_doc(&interpolate(doc, lookup)?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest(phone: Option<&str>) -> Manifest {
        let mut doc = json!({ "apiVersion": "paloma/v1", "kind": "Calling", "spec": {} });
        if let Some(phone) = phone {
            doc["metadata"] = json!({ "phoneNumberId": phone });
        }
        Manifest::from_doc(&doc).unwrap()
    }

    #[test]
    fn a_run_flag_beats_the_document_which_beats_the_profile() {
        let base = Scope::new(Some("w"), Some("profile"), None);
        assert_eq!(
            base.for_document(&manifest(Some("doc")), Some("flag"))
                .phone_number_id(),
            Some("flag")
        );
        assert_eq!(
            base.for_document(&manifest(Some("doc")), None)
                .phone_number_id(),
            Some("doc")
        );
        assert_eq!(
            base.for_document(&manifest(None), None).phone_number_id(),
            Some("profile")
        );
    }

    #[test]
    fn no_phone_number_anywhere_stays_missing_rather_than_empty() {
        let scope = Scope::new(Some("w"), None, None).for_document(&manifest(None), None);
        assert!(scope.require_phone_number_id().is_err());
        assert_eq!(scope.label(Kind::Calling), "Calling/?");
        assert_eq!(scope.label(Kind::Template), "Template/w");
    }
}
