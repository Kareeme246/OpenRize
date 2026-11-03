//! Shared HTTP client setup.

/// reqwest is built without a bundled crypto provider (`rustls-no-provider`),
/// so every HTTPS client needs one installed first. Installing is process
/// wide and idempotent; call this before building any client.
pub fn ensure_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}
