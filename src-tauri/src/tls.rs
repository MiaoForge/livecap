/// Reqwest enables aws-lc-rs while the WebSocket stack enables ring. Rustls
/// cannot infer a default when both are present, so select one before any
/// provider connection or updater client is created.
pub fn initialize() {
    // Another caller may already have initialized this process. Installation is
    // process-wide and first-writer-wins; retaining that provider is intentional.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

#[cfg(test)]
mod tests {
    #[test]
    fn initializes_tls_with_both_crypto_backends_enabled() {
        super::initialize();
        let provider = rustls::crypto::CryptoProvider::get_default().unwrap();
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
        rustls::ClientConnection::new(
            std::sync::Arc::new(config),
            rustls::pki_types::ServerName::try_from("localhost").unwrap(),
        )
        .unwrap();
        assert!(!provider.cipher_suites.is_empty());
        super::initialize();
        assert!(std::ptr::eq(
            provider,
            rustls::crypto::CryptoProvider::get_default().unwrap()
        ));
    }
}
