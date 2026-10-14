use fetchkit::DocumentConversionSettings;

fn assert_redacted(settings: &DocumentConversionSettings) {
    for output in [format!("{settings:?}"), format!("{settings:#?}")] {
        assert!(output.contains("<redacted>"));
        assert!(!output.contains("dummy-credential"));
        for visible in [
            "formats",
            "pdf",
            "docx",
            "converters",
            "cloudflare",
            "allow_remote: true",
            "max_document_bytes",
            "42",
            "cloudflare_account_id",
            "test-account",
            "cloudflare_api_token",
            "respect_proxy_env: true",
        ] {
            assert!(
                output.contains(visible),
                "missing nonsecret setting: {visible}"
            );
        }
    }
}

#[test]
fn settings_debug_redacts_before_and_after_build_and_clone() {
    let settings = DocumentConversionSettings {
        formats: Some(vec!["pdf".into(), "docx".into()]),
        converters: Some(vec!["cloudflare".into()]),
        allow_remote: true,
        max_document_bytes: Some(42),
        cloudflare_account_id: Some("test-account".into()),
        cloudflare_api_token: Some("dummy-credential".into()),
        respect_proxy_env: true,
    };
    assert_redacted(&settings);
    let result = settings.build();
    #[cfg(feature = "docs-cloudflare")]
    assert!(result.is_ok());
    #[cfg(not(feature = "docs-cloudflare"))]
    assert_eq!(
        result.unwrap_err(),
        "converter `cloudflare` requires the docs-cloudflare feature"
    );
    assert_redacted(&settings);
    let cloned = settings.clone();
    assert_eq!(
        cloned.cloudflare_api_token.as_deref(),
        Some("dummy-credential")
    );
    assert_redacted(&cloned);
}

#[test]
fn settings_debug_distinguishes_absent_and_empty_credentials() {
    let mut settings = DocumentConversionSettings::default();
    for output in [format!("{settings:?}"), format!("{settings:#?}")] {
        assert!(output.contains("cloudflare_api_token: None"));
        assert!(!output.contains("<redacted>"));
        assert!(output.contains("allow_remote: false"));
        assert!(output.contains("respect_proxy_env: false"));
    }
    settings.cloudflare_api_token = Some(String::new());
    for output in [format!("{settings:?}"), format!("{settings:#?}")] {
        assert!(output.contains("<redacted>"));
        assert!(!output.contains("cloudflare_api_token: None"));
    }
}
