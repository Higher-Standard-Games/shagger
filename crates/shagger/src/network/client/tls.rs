//! The TLS setup for `wss://` connections to the lobby and game servers. It
//! has no TypeScript counterpart: the browser brings its own.
//!
//! The client trusts the Mozilla roots bundled by webpki-roots, so it works
//! the same on every machine. Like Node's `NODE_EXTRA_CA_CERTS`, the PEM
//! certificates in the file `SSL_CERT_FILE` names are trusted as well, for
//! networks that re-sign TLS through a proxy of their own.

use std::sync::{Arc, OnceLock};

use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;
use rustls::{ClientConfig, RootCertStore};
use tokio_tungstenite::Connector;

/// The environment variable naming extra trusted certificates.
pub const EXTRA_CERTS_VAR: &str = "SSL_CERT_FILE";

fn config() -> Arc<ClientConfig> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            if let Some(file) = std::env::var_os(EXTRA_CERTS_VAR) {
                // A file that cannot be read adds nothing; the bundled roots still apply.
                if let Ok(certs) = CertificateDer::pem_file_iter(&file) {
                    let certs: Vec<_> = certs.filter_map(Result::ok).collect();
                    roots.add_parsable_certificates(certs);
                }
            }
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("ring supports the default protocol versions")
                .with_root_certificates(roots)
                .with_no_client_auth();
            Arc::new(config)
        })
        .clone()
}

/// The connector for `connect_async_tls_with_config`.
#[must_use]
pub fn connector() -> Connector {
    Connector::Rustls(config())
}
