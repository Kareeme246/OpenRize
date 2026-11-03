/// reqwest is built without a crypto provider (`rustls-no-provider`); install one before
/// building any HTTPS client. Process-wide and idempotent.
pub fn ensure_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}
