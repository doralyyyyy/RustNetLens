use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ::ring::digest::{SHA256, digest};
use chrono::{DateTime, Utc};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedKey, DistinguishedName, DnType, IsCa, Issuer,
    KeyPair, KeyUsagePurpose,
};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::ring;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey as RustlsCertifiedKey;
use tokio::sync::{Mutex, RwLock};

use crate::error::SecurityError;
use crate::model::{HttpsMitmStatus, RootCaInfo};

#[derive(Debug, Clone)]
pub struct RootCaBundle {
    pub generated_at: DateTime<Utc>,
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub cert_pem: String,
    pub key_pem: String,
}

impl RootCaBundle {
    pub fn info(&self) -> RootCaInfo {
        RootCaInfo {
            generated_at: self.generated_at,
            cert_path: self.cert_path.to_string_lossy().to_string(),
            fingerprint_sha256: sha256_hex(self.cert_pem.as_bytes()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpsMitmState {
    enabled: bool,
    local_only: bool,
    bundle: Option<RootCaBundle>,
    leaf_cache: Arc<RwLock<BTreeMap<String, Arc<RustlsCertifiedKey>>>>,
}

impl Default for HttpsMitmState {
    fn default() -> Self {
        Self {
            enabled: false,
            local_only: true,
            bundle: None,
            leaf_cache: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }
}

impl HttpsMitmState {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn is_ready(&self) -> bool {
        self.bundle.is_some()
    }

    pub fn status(&self) -> HttpsMitmStatus {
        HttpsMitmStatus {
            enabled: self.enabled,
            ready: self.is_ready(),
            local_only: self.local_only,
            default_off: true,
            root_ca: self.bundle.as_ref().map(|bundle| bundle.info()),
            install_hint: if let Some(bundle) = self.bundle.as_ref() {
                format!(
                    "Local Root CA is ready. Install {} on devices you control, then enable HTTPS decrypt explicitly.",
                    bundle.cert_path.display()
                )
            } else {
                "HTTPS decrypt is disabled by default. Generate a local Root CA before enabling it, and install that CA only on machines you control.".into()
            },
        }
    }

    pub fn ensure_root_ca(&mut self, dir: &Path) -> Result<RootCaInfo, SecurityError> {
        if self.bundle.is_none() {
            self.bundle = load_root_ca(dir).or_else(|| generate_root_ca(dir).ok());
        }
        self.bundle
            .as_ref()
            .map(|bundle| bundle.info())
            .ok_or_else(|| SecurityError::Certificate("failed to load or generate root CA".into()))
    }

    pub fn root_ca_paths(&self) -> Option<(PathBuf, PathBuf)> {
        self.bundle
            .as_ref()
            .map(|bundle| (bundle.cert_path.clone(), bundle.key_path.clone()))
    }

    pub fn root_ca_pem(&self) -> Option<(String, String)> {
        self.bundle
            .as_ref()
            .map(|bundle| (bundle.cert_pem.clone(), bundle.key_pem.clone()))
    }

    pub fn root_ca_info(&self) -> Option<RootCaInfo> {
        self.bundle.as_ref().map(|bundle| bundle.info())
    }

    pub async fn resolve_server_cert(
        &self,
        server_name: &str,
    ) -> Result<Arc<RustlsCertifiedKey>, SecurityError> {
        if let Some(cached) = self.leaf_cache.read().await.get(server_name).cloned() {
            return Ok(cached);
        }
        let bundle = self
            .bundle
            .as_ref()
            .ok_or_else(|| SecurityError::Certificate("root CA missing".into()))?;
        let leaf = generate_leaf_cert(server_name, bundle)?;
        let certified = Arc::new(to_rustls_certified_key(leaf)?);
        self.leaf_cache
            .write()
            .await
            .insert(server_name.to_string(), certified.clone());
        Ok(certified)
    }
}

pub fn load_root_ca(dir: &Path) -> Option<RootCaBundle> {
    let cert_path = dir.join("rustnetlens-root-ca.pem");
    let key_path = dir.join("rustnetlens-root-ca-key.pem");
    if !cert_path.exists() || !key_path.exists() {
        return None;
    }
    let cert_pem = fs::read_to_string(&cert_path).ok()?;
    let key_pem = fs::read_to_string(&key_path).ok()?;
    let generated_at = fs::metadata(&cert_path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(Utc::now);
    Some(RootCaBundle {
        generated_at,
        cert_path,
        key_path,
        cert_pem,
        key_pem,
    })
}

pub fn generate_root_ca(dir: &Path) -> Result<RootCaBundle, SecurityError> {
    fs::create_dir_all(dir).map_err(|e| SecurityError::Io(e.to_string()))?;
    let mut params = CertificateParams::new(Vec::new())
        .map_err(|e| SecurityError::Certificate(e.to_string()))?;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "RustNetLens Local Root CA");
    let signing_key = KeyPair::generate().map_err(|e| SecurityError::Certificate(e.to_string()))?;
    let cert = params
        .self_signed(&signing_key)
        .map_err(|e| SecurityError::Certificate(e.to_string()))?;
    let cert_pem = cert.pem();
    let key_pem = signing_key.serialize_pem();
    let cert_path = dir.join("rustnetlens-root-ca.pem");
    let key_path = dir.join("rustnetlens-root-ca-key.pem");
    fs::write(&cert_path, &cert_pem).map_err(|e| SecurityError::Io(e.to_string()))?;
    fs::write(&key_path, &key_pem).map_err(|e| SecurityError::Io(e.to_string()))?;
    Ok(RootCaBundle {
        generated_at: Utc::now(),
        cert_path,
        key_path,
        cert_pem,
        key_pem,
    })
}

fn generate_leaf_cert(
    server_name: &str,
    bundle: &RootCaBundle,
) -> Result<CertifiedKey<KeyPair>, SecurityError> {
    let mut params = CertificateParams::new(vec![server_name.to_string()])
        .map_err(|e| SecurityError::Certificate(e.to_string()))?;
    params
        .distinguished_name
        .push(DnType::CommonName, server_name);
    params.use_authority_key_identifier_extension = true;
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    let signing_key = KeyPair::generate().map_err(|e| SecurityError::Certificate(e.to_string()))?;
    let issuer_key = KeyPair::from_pem(&bundle.key_pem)
        .map_err(|e| SecurityError::Certificate(e.to_string()))?;
    let issuer = Issuer::from_ca_cert_pem(&bundle.cert_pem, issuer_key)
        .map_err(|e| SecurityError::Certificate(e.to_string()))?;
    let cert = params
        .signed_by(&signing_key, &issuer)
        .map_err(|e| SecurityError::Certificate(e.to_string()))?;
    Ok(CertifiedKey { cert, signing_key })
}

fn to_rustls_certified_key(
    cert: CertifiedKey<KeyPair>,
) -> Result<RustlsCertifiedKey, SecurityError> {
    let certs = vec![CertificateDer::from(cert.cert.der().to_vec())];
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()));
    let provider = ring::default_provider();
    RustlsCertifiedKey::from_der(certs, key, &provider)
        .map_err(|e| SecurityError::Tls(e.to_string()))
}

fn sha256_hex(input: &[u8]) -> String {
    let hash = digest(&SHA256, input);
    hash.as_ref().iter().map(|b| format!("{:02x}", b)).collect()
}

#[derive(Debug, Clone)]
pub struct MitmCertResolver {
    state: Arc<Mutex<HttpsMitmState>>,
}

impl MitmCertResolver {
    pub fn new(state: Arc<Mutex<HttpsMitmState>>) -> Self {
        Self { state }
    }
}

impl ResolvesServerCert for MitmCertResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<RustlsCertifiedKey>> {
        let server_name = client_hello.server_name()?;
        let state = self.state.try_lock().ok()?;
        let bundle = state.bundle.as_ref()?;
        let leaf = generate_leaf_cert(server_name, bundle).ok()?;
        to_rustls_certified_key(leaf).ok().map(Arc::new)
    }
}

pub fn build_mitm_server_config(
    state: Arc<Mutex<HttpsMitmState>>,
) -> Result<Arc<rustls::ServerConfig>, SecurityError> {
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(MitmCertResolver::new(state)));
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

#[derive(Debug)]
pub struct MitmClientVerifier;

impl ServerCertVerifier for MitmClientVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        use rustls::SignatureScheme::*;
        vec![
            ECDSA_NISTP256_SHA256,
            ECDSA_NISTP384_SHA384,
            RSA_PSS_SHA256,
            RSA_PSS_SHA384,
            RSA_PSS_SHA512,
            RSA_PKCS1_SHA256,
            RSA_PKCS1_SHA384,
            RSA_PKCS1_SHA512,
        ]
    }
}
