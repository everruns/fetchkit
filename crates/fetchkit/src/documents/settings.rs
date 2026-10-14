// Decisions: Preserve string-based settings across all feature sets; redact only
// the credential in Debug without changing public storage or build/clone semantics.
//! String-based document conversion settings shared by the CLI, MCP server and
//! Python bindings. Libraries can use [`DocumentConversion`] directly instead.

use std::fmt;

use super::{DocumentConversion, DocumentFormat};

/// Built-in converter names accepted by [`DocumentConversionSettings`].
///
/// `office` is the deprecated in-crate Office converter (`docs-office` feature);
/// `markkit` is the default Office backend.
pub const BUILTIN_CONVERTERS: &[&str] = &["pdf-inspector", "markkit", "office", "cloudflare"];

/// Plain settings resolved into a [`DocumentConversion`].
///
/// Credentials are passed explicitly; nothing is read from the environment.
#[derive(Default, Clone)]
pub struct DocumentConversionSettings {
    /// Accepted formats (e.g. `["pdf", "docx"]`). `None` keeps every supported format.
    pub formats: Option<Vec<String>>,
    /// Converter order applied to every format (e.g. `["markkit", "cloudflare"]`).
    /// `None` keeps the built-in local converters in their default order.
    pub converters: Option<Vec<String>>,
    /// Allow converters that send documents to another service.
    pub allow_remote: bool,
    /// Maximum downloaded document size in bytes.
    pub max_document_bytes: Option<usize>,
    /// Cloudflare account id, required when `converters` names `cloudflare`.
    pub cloudflare_account_id: Option<String>,
    /// Cloudflare API token with Workers AI permission.
    /// Redacted in settings `Debug`; direct field access still exposes the value.
    pub cloudflare_api_token: Option<String>,
    /// Honour proxy environment variables for remote converter uploads.
    pub respect_proxy_env: bool,
}

impl fmt::Debug for DocumentConversionSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // THREAT[TM-LEAK-007]: Never format the original credential or a clone of it.
        f.debug_struct("DocumentConversionSettings")
            .field("formats", &self.formats)
            .field("converters", &self.converters)
            .field("allow_remote", &self.allow_remote)
            .field("max_document_bytes", &self.max_document_bytes)
            .field("cloudflare_account_id", &self.cloudflare_account_id)
            .field(
                "cloudflare_api_token",
                &self.cloudflare_api_token.as_ref().map(|_| "<redacted>"),
            )
            .field("respect_proxy_env", &self.respect_proxy_env)
            .finish()
    }
}

impl DocumentConversionSettings {
    /// Whether every field is at its default, i.e. nothing to configure.
    pub fn is_default(&self) -> bool {
        self.formats.is_none()
            && self.converters.is_none()
            && !self.allow_remote
            && self.max_document_bytes.is_none()
    }

    /// Resolve the settings. Errors name the offending value.
    pub fn build(&self) -> Result<DocumentConversion, String> {
        let mut conversion = DocumentConversion::default().allow_remote(self.allow_remote);

        if let Some(formats) = &self.formats {
            let formats = formats
                .iter()
                .map(|value| value.parse::<DocumentFormat>())
                .collect::<Result<Vec<_>, _>>()?;
            conversion = conversion.formats(formats);
        }
        if let Some(bytes) = self.max_document_bytes {
            conversion = conversion.max_document_bytes(bytes);
        }

        if let Some(names) = &self.converters {
            let names: Vec<String> = names
                .iter()
                .map(|name| name.trim().to_ascii_lowercase())
                .filter(|name| !name.is_empty())
                .collect();
            if let Some(unknown) = names
                .iter()
                .find(|name| !BUILTIN_CONVERTERS.contains(&name.as_str()))
            {
                return Err(format!(
                    "unknown converter `{unknown}` (known: {})",
                    BUILTIN_CONVERTERS.join(", ")
                ));
            }
            for name in &names {
                conversion = self.add_builtin(conversion, name)?;
            }
            for format in DocumentFormat::ALL {
                conversion = conversion.chain(*format, names.iter().cloned());
            }
        }
        Ok(conversion)
    }

    fn add_builtin(
        &self,
        conversion: DocumentConversion,
        name: &str,
    ) -> Result<DocumentConversion, String> {
        match name {
            "pdf-inspector" => {
                if cfg!(feature = "docs-pdf") {
                    Ok(conversion)
                } else {
                    Err("converter `pdf-inspector` requires the docs-pdf feature".to_string())
                }
            }
            "markkit" => {
                if cfg!(feature = "docs-markkit") {
                    Ok(conversion)
                } else {
                    Err("converter `markkit` requires the docs-markkit feature".to_string())
                }
            }
            "office" => {
                if cfg!(feature = "docs-office") {
                    Ok(conversion)
                } else {
                    Err("converter `office` (deprecated; use `markkit`) requires the docs-office feature".to_string())
                }
            }
            "cloudflare" => self.cloudflare(conversion),
            _ => unreachable!("validated against BUILTIN_CONVERTERS"),
        }
    }

    #[cfg(feature = "docs-cloudflare")]
    fn cloudflare(&self, conversion: DocumentConversion) -> Result<DocumentConversion, String> {
        use std::sync::Arc;

        if !self.allow_remote {
            return Err(
                "converter `cloudflare` uploads documents to Cloudflare; allow remote conversion to use it"
                    .to_string(),
            );
        }
        let (Some(account_id), Some(api_token)) = (
            self.cloudflare_account_id
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
            self.cloudflare_api_token
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
        ) else {
            return Err(
                "converter `cloudflare` needs a Cloudflare account id and API token".to_string(),
            );
        };
        let client = super::ReqwestConverterClient::new().respect_proxy_env(self.respect_proxy_env);
        Ok(conversion.converter(
            super::CloudflareConverter::new(account_id.trim(), api_token.trim())
                .client(Arc::new(client)),
        ))
    }

    #[cfg(not(feature = "docs-cloudflare"))]
    fn cloudflare(&self, _conversion: DocumentConversion) -> Result<DocumentConversion, String> {
        Err("converter `cloudflare` requires the docs-cloudflare feature".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_names_and_formats() {
        let settings = DocumentConversionSettings {
            converters: Some(vec!["magic".into()]),
            ..Default::default()
        };
        assert!(settings
            .build()
            .unwrap_err()
            .contains("unknown converter `magic`"));
        let settings = DocumentConversionSettings {
            formats: Some(vec!["exe".into()]),
            ..Default::default()
        };
        assert!(settings.build().unwrap_err().contains("exe"));
    }

    #[test]
    fn formats_and_limits_apply() {
        let settings = DocumentConversionSettings {
            formats: Some(vec!["docx".into()]),
            max_document_bytes: Some(10),
            ..Default::default()
        };
        assert!(!settings.is_default());
        let conversion = settings.build().unwrap();
        assert!(conversion.chain_for(DocumentFormat::Pdf).is_empty());
    }

    #[cfg(feature = "docs-cloudflare")]
    #[test]
    fn cloudflare_requires_remote_opt_in_and_credentials() {
        let mut settings = DocumentConversionSettings {
            converters: Some(vec!["cloudflare".into()]),
            ..Default::default()
        };
        assert!(settings.build().unwrap_err().contains("allow remote"));
        settings.allow_remote = true;
        assert!(settings.build().unwrap_err().contains("account id"));
        settings.cloudflare_account_id = Some("acct".into());
        settings.cloudflare_api_token = Some("tok".into());
        let conversion = settings.build().unwrap();
        assert_eq!(conversion.chain_for(DocumentFormat::Docx).len(), 1);
        assert!(conversion.chain_for(DocumentFormat::Pptx).is_empty());
    }

    #[cfg(feature = "docs-markkit")]
    #[test]
    fn markkit_converter_is_selectable_by_name() {
        let settings = DocumentConversionSettings {
            converters: Some(vec!["markkit".into()]),
            ..Default::default()
        };
        let conversion = settings.build().unwrap();
        let names = |format| {
            conversion
                .chain_for(format)
                .iter()
                .map(|converter| converter.name().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(DocumentFormat::Docx), ["markkit"]);
        assert_eq!(names(DocumentFormat::Xlsx), ["markkit"]);
        assert!(names(DocumentFormat::Pdf).is_empty());
    }

    #[cfg(not(feature = "docs-markkit"))]
    #[test]
    fn markkit_without_feature_is_explained() {
        let settings = DocumentConversionSettings {
            converters: Some(vec!["markkit".into()]),
            ..Default::default()
        };
        assert!(settings
            .build()
            .unwrap_err()
            .contains("requires the docs-markkit feature"));
    }

    #[cfg(not(feature = "docs-office"))]
    #[test]
    fn office_without_feature_points_to_markkit() {
        let settings = DocumentConversionSettings {
            converters: Some(vec!["office".into()]),
            ..Default::default()
        };
        let error = settings.build().unwrap_err();
        assert!(error.contains("docs-office feature"), "{error}");
        assert!(error.contains("markkit"), "{error}");
    }

    #[cfg(not(feature = "docs-cloudflare"))]
    #[test]
    fn cloudflare_without_feature_is_explained() {
        let settings = DocumentConversionSettings {
            converters: Some(vec!["cloudflare".into()]),
            allow_remote: true,
            ..Default::default()
        };
        assert!(settings
            .build()
            .unwrap_err()
            .contains("docs-cloudflare feature"));
    }
}
