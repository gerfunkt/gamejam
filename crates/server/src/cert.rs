//! A self-signed certificate for `localhost` and every address of the
//! machine, written by rcgen with a pure-Rust ECDSA P-256 key from
//! p256. It is kept in `.server/` until the addresses change.

use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use p256::pkcs8::{EncodePrivateKey, LineEnding};
use p256::pkcs8::DecodePrivateKey;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, RemoteKeyPair, SanType, SerialNumber};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::io;
use std::net::IpAddr;
use std::path::Path;

struct P256Key {
    key: SigningKey,
    public: Vec<u8>,
}

impl RemoteKeyPair for P256Key {
    fn public_key(&self) -> &[u8] {
        &self.public
    }

    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, rcgen::Error> {
        let signature: Signature = self.key.sign(msg);
        Ok(signature.to_der().as_bytes().to_vec())
    }

    fn algorithm(&self) -> &'static rcgen::SignatureAlgorithm {
        &rcgen::PKCS_ECDSA_P256_SHA256
    }
}

fn p256_to_rcgen(key: SigningKey) -> Result<KeyPair, rcgen::Error> {
    let public = key
        .verifying_key()
        .to_encoded_point(false)
        .as_bytes()
        .to_vec();
    KeyPair::from_remote(Box::new(P256Key { key, public }))
}

/// Every name the certificate covers: localhost and each address of
/// the machine, sorted so the cache check is stable.
pub fn san_list() -> Vec<SanType> {
    let mut ips: Vec<IpAddr> = vec!["127.0.0.1".parse().unwrap(), "::1".parse().unwrap()];
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for interface in interfaces {
            if interface.is_loopback() {
                continue;
            }
            let ip = interface.ip();
            if !ips.contains(&ip) {
                ips.push(ip);
            }
        }
    }
    ips.sort();
    let mut sans = vec![SanType::DnsName("localhost".try_into().unwrap())];
    sans.extend(ips.into_iter().map(SanType::IpAddress));
    sans
}

/// A description of the SAN list, for the cache check.
fn san_stamp(sans: &[SanType]) -> String {
    sans.iter()
        .map(|san| match san {
            SanType::DnsName(name) => format!("dns:{}", name.as_str()),
            SanType::IpAddress(ip) => format!("ip:{ip}"),
            _ => "other".into(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub struct TlsParts {
    pub chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
}

/// Load the cached certificate, or write a new one when the addresses
/// of the machine changed.
pub fn load_or_create(dir: &Path) -> io::Result<TlsParts> {
    std::fs::create_dir_all(dir)?;
    let sans = san_list();
    let stamp = san_stamp(&sans);
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    let stamp_path = dir.join("stamp");

    if std::fs::read_to_string(&stamp_path).ok().as_deref() == Some(stamp.as_str()) {
        if let (Ok(cert_pem), Ok(key_pem)) =
            (std::fs::read_to_string(&cert_path), std::fs::read_to_string(&key_path))
        {
            if let Some(parts) = parse(&cert_pem, &key_pem) {
                return Ok(parts);
            }
        }
    }

    let mut params = CertificateParams::new(Vec::new()).map_err(err)?;
    params.serial_number = Some(SerialNumber::from(1u64));
    params.subject_alt_names = sans;
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "gamejam LAN server");
    let key = SigningKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
    let rcgen_key = p256_to_rcgen(key.clone()).map_err(err)?;
    let cert = params.self_signed(&rcgen_key).map_err(err)?;

    let cert_pem = cert.pem();
    let key_pem = key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|e| err(io::Error::new(io::ErrorKind::Other, e)))?;
    std::fs::write(&cert_path, &cert_pem)?;
    std::fs::write(&key_path, &*key_pem)?;
    std::fs::write(&stamp_path, stamp)?;

    parse(&cert_pem, &key_pem)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "fresh certificate did not parse"))
}

fn parse(cert_pem: &str, key_pem: &str) -> Option<TlsParts> {
    let chain = rustls_pemfile::certs(&mut cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let key: SigningKey = DecodePrivateKey::from_pkcs8_pem(key_pem).ok()?;
    let der = key.to_pkcs8_der().ok()?;
    Some(TlsParts {
        chain,
        key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(der.as_bytes().to_vec())),
    })
}

fn err(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}
