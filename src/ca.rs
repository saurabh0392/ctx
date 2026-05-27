//! Local certificate authority for HTTPS MITM (api.anthropic.com only).

use std::collections::HashMap;
use std::io::{BufReader, Cursor};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer,
    KeyPair, KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::sign::{CertifiedKey, SingleCertAndKey};

use crate::config;

/// Hostname the proxy terminates TLS for (Claude Code still uses first-party OAuth).
pub const ANTHROPIC_API_HOST: &str = "api.anthropic.com";

pub fn ca_cert_path() -> std::path::PathBuf {
    config::ctx_dir().join("ca-cert.pem")
}

pub fn ca_key_path() -> std::path::PathBuf {
    config::ctx_dir().join("ca-key.pem")
}

pub fn canonical_ca_cert_path_string() -> Result<String> {
    ensure_ca()?;
    let p = ca_cert_path();
    let p = if p.exists() {
        std::fs::canonicalize(&p).unwrap_or(p)
    } else {
        p
    };
    p.to_str()
        .map(|s| s.to_string())
        .context("ca-cert path is not valid UTF-8")
}

/// Ensure `~/.ctx/ca-cert.pem` and `ca-key.pem` exist (generate on first run).
pub fn ensure_ca() -> Result<()> {
    config::ensure_dir()?;
    let cert_path = ca_cert_path();
    let key_path = ca_key_path();
    if cert_path.exists() && key_path.exists() {
        return Ok(());
    }
    let ca_key = KeyPair::generate()?;
    let mut ca_params = CertificateParams::new(vec!["ctx-proxy-local-ca".to_string()])?;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_cert = ca_params.self_signed(&ca_key)?;
    std::fs::write(&cert_path, ca_cert.pem())?;
    std::fs::write(&key_path, ca_key.serialize_pem())?;
    Ok(())
}

/// Loads or creates CA material, issues leaf certs for MITM, caches by hostname.
pub struct CertAuthority {
    issuer: Issuer<'static, KeyPair>,
    cache: Mutex<HashMap<String, Arc<CertifiedKey>>>,
}

impl CertAuthority {
    pub fn load_or_generate() -> Result<Arc<Self>> {
        ensure_ca()?;
        let cert_pem = std::fs::read_to_string(ca_cert_path()).context("read ca-cert.pem")?;
        let key_bytes = std::fs::read(ca_key_path()).context("read ca-key.pem")?;
        let mut key_reader = Cursor::new(key_bytes);
        let key_der = rustls_pemfile::private_key(&mut key_reader)
            .context("parse ca-key.pem")?
            .context("no PKCS8 private key in ca-key.pem")?;
        let ca_key = KeyPair::try_from(&key_der).map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut cert_reader = BufReader::new(Cursor::new(cert_pem.as_bytes()));
        let ca_der = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .context("parse ca-cert.pem")?
            .into_iter()
            .next()
            .context("no certificate in ca-cert.pem")?;
        let issuer = Issuer::from_ca_cert_der(&ca_der.into(), ca_key)
            .map_err(|e| anyhow::anyhow!("issuer from CA: {e}"))?;
        Ok(Arc::new(Self {
            issuer,
            cache: Mutex::new(HashMap::new()),
        }))
    }

    /// Returns a rustls [`CertifiedKey`] for TLS server handshake (cached per hostname).
    pub fn issue_certified_key(&self, hostname: &str) -> Result<Arc<CertifiedKey>> {
        {
            let g = self.cache.lock().unwrap();
            if let Some(k) = g.get(hostname) {
                return Ok(Arc::clone(k));
            }
        }
        let leaf_key = KeyPair::generate()?;
        let mut leaf_params = CertificateParams::new(vec![hostname.to_string()])?;
        leaf_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let leaf_cert: Certificate = leaf_params
            .signed_by(&leaf_key, &self.issuer)
            .map_err(|e| anyhow::anyhow!("sign leaf: {e}"))?;
        let cert_der = CertificateDer::from(leaf_cert.der().as_ref().to_vec());
        let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));
        let provider = rustls::crypto::CryptoProvider::get_default().context("rustls CryptoProvider not installed")?;
        let ck = CertifiedKey::from_der(vec![cert_der], key_der, provider.as_ref())
            .map_err(|e| anyhow::anyhow!("CertifiedKey::from_der: {e}"))?;
        let arc = Arc::new(ck);
        let mut g = self.cache.lock().unwrap();
        if g.len() > 64 {
            g.clear();
        }
        g.insert(hostname.to_string(), Arc::clone(&arc));
        Ok(arc)
    }

    /// Builds a [`rustls::ServerConfig`] that presents a leaf for `ANTHROPIC_API_HOST`.
    pub fn server_config_for_anthropic(&self) -> Result<Arc<rustls::ServerConfig>> {
        let ck = self.issue_certified_key(ANTHROPIC_API_HOST)?;
        let mut cfg = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(SingleCertAndKey::from(ck)));
        cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(cfg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ca_generates_and_leaf_is_signed() {
        let _g = crate::test_lock::CTX_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::ensure_tls_crypto_provider();
        let prev = std::env::var("CTX_HOME").ok();
        let dir = tempdir().unwrap();
        std::env::set_var("CTX_HOME", dir.path());
        ensure_ca().unwrap();
        let pem = std::fs::read_to_string(ca_cert_path()).unwrap();
        assert!(pem.contains("BEGIN CERTIFICATE"));
        let authority = CertAuthority::load_or_generate().unwrap();
        let leaf = authority.issue_certified_key(ANTHROPIC_API_HOST).unwrap();
        assert_eq!(leaf.cert.len(), 1);
        match prev {
            Some(v) => std::env::set_var("CTX_HOME", v),
            None => std::env::remove_var("CTX_HOME"),
        }
    }
}
