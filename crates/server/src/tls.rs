//! The certificate of the server, and the TLS settings that use it.
//!
//! Everything here is pure Rust: `rcgen` writes the certificate, `p256` signs it, and
//! `rustls-rustcrypto` is the cryptography of `rustls`.

use std::fs;
use std::io::Write;
use std::net::Ipv4Addr;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::Generate;
use p256::pkcs8::EncodePrivateKey;
use p256::pkcs8::LineEnding;
use rcgen::{CertificateParams, PKCS_ECDSA_P256_SHA256, PublicKeyData, SerialNumber, SignatureAlgorithm};
use rustls::ServerConfig;
use rustls::crypto::CryptoProvider;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

/// The names of the certificate for the machine, apart from its addresses.
const HOST_NAME: &str = "localhost";

/// A certificate chain and the private key that belongs to it.
#[derive(Debug)]
pub struct Identity {
	certificates: Vec<CertificateDer<'static>>,
	key: PrivateKeyDer<'static>,
}

impl Identity {
	/// Read a certificate chain and a private key from two PEM files.
	///
	/// # Errors
	///
	/// Returns an error when a file is missing or holds no certificate or key.
	pub fn load(certificate: &Path, key: &Path) -> Result<Self> {
		let certificates = CertificateDer::pem_file_iter(certificate)
			.with_context(|| format!("cannot read the certificate {}", certificate.display()))?
			.collect::<Result<Vec<_>, _>>()
			.with_context(|| format!("{} holds no valid certificate", certificate.display()))?;
		anyhow::ensure!(
			!certificates.is_empty(),
			"{} holds no certificate",
			certificate.display()
		);
		let key =
			PrivateKeyDer::from_pem_file(key).with_context(|| format!("cannot read the key {}", key.display()))?;
		Ok(Self { certificates, key })
	}

	/// Return the certificate that `dir` keeps for exactly `addresses`, or make and keep a new one.
	///
	/// The certificate names `localhost` and each address. `dir` holds `cert.pem`, `key.pem`, and
	/// `addresses`, which lists the addresses of the certificate. A new certificate replaces the
	/// old one only when the list differs.
	///
	/// # Errors
	///
	/// Returns an error when the certificate cannot be made or written.
	pub fn load_or_create(dir: &Path, addresses: &[Ipv4Addr]) -> Result<Self> {
		let mut addresses = addresses.to_vec();
		addresses.sort();
		addresses.dedup();
		let list: String = addresses.iter().map(|address| format!("{address}\n")).collect();
		if fs::read_to_string(dir.join("addresses")).is_ok_and(|kept| kept == list)
			&& let Ok(identity) = Self::load(&dir.join("cert.pem"), &dir.join("key.pem"))
		{
			return Ok(identity);
		}
		let key = SigningKey::generate();
		let names = std::iter::once(HOST_NAME.to_owned()).chain(addresses.iter().map(ToString::to_string));
		let mut params = CertificateParams::new(names.collect::<Vec<_>>()).context("cannot name the certificate")?;
		// Without the `crypto` feature of `rcgen`, which needs `ring`, the serial number is ours to set.
		let mut serial = [0_u8; 20];
		getrandom::fill(&mut serial).context("the system has no random numbers")?;
		serial[0] &= 0x7f;
		params.serial_number = Some(SerialNumber::from_slice(&serial));
		let certificate = params
			.self_signed(&P256Key::new(&key))
			.context("cannot make the certificate")?;
		fs::create_dir_all(dir).with_context(|| format!("cannot make the directory {}", dir.display()))?;
		let key_pem = key.to_pkcs8_pem(LineEnding::LF).context("cannot write the key")?;
		write(&dir.join("cert.pem"), certificate.pem().as_bytes(), 0o644)?;
		write(&dir.join("key.pem"), key_pem.as_bytes(), 0o600)?;
		write(&dir.join("addresses"), list.as_bytes(), 0o644)?;
		Self::load(&dir.join("cert.pem"), &dir.join("key.pem"))
	}

	/// Return the TLS settings of a server that presents this certificate, over HTTP/1.1 only.
	///
	/// # Errors
	///
	/// Returns an error when the key does not belong to the certificate, or the provider cannot
	/// use it.
	pub fn server_config(self) -> Result<ServerConfig> {
		let mut config = ServerConfig::builder_with_provider(provider())
			.with_safe_default_protocol_versions()
			.context("the provider supports no protocol version")?
			.with_no_client_auth()
			.with_single_cert(self.certificates, self.key)
			.context("the key does not fit the certificate")?;
		config.alpn_protocols = vec![b"http/1.1".to_vec()];
		Ok(config)
	}
}

/// Return the pure-Rust cryptography that every TLS setting of the server uses.
#[must_use]
pub fn provider() -> Arc<CryptoProvider> {
	Arc::new(rustls_rustcrypto::provider())
}

/// Return the IPv4 addresses of the machine, loopback included. The server listens on IPv4.
///
/// # Errors
///
/// Returns an error when the system cannot list its interfaces.
pub fn machine_addresses() -> Result<Vec<Ipv4Addr>> {
	let interfaces = if_addrs::get_if_addrs().context("cannot list the network interfaces")?;
	let mut addresses: Vec<Ipv4Addr> = interfaces
		.into_iter()
		.filter_map(|interface| match interface.ip() {
			std::net::IpAddr::V4(address) => Some(address),
			std::net::IpAddr::V6(_) => None,
		})
		.collect();
	addresses.sort();
	addresses.dedup();
	Ok(addresses)
}

fn write(path: &Path, content: &[u8], mode: u32) -> Result<()> {
	fs::OpenOptions::new()
		.write(true)
		.create(true)
		.truncate(true)
		.mode(mode)
		.open(path)
		.and_then(|mut file| file.write_all(content))
		.with_context(|| format!("cannot write {}", path.display()))
}

/// A P-256 key that signs a certificate for `rcgen`.
struct P256Key<'a> {
	key: &'a SigningKey,
	public: Vec<u8>,
}

impl<'a> P256Key<'a> {
	fn new(key: &'a SigningKey) -> Self {
		let public = key.verifying_key().to_sec1_point(false).as_bytes().to_vec();
		Self { key, public }
	}
}

impl PublicKeyData for P256Key<'_> {
	fn der_bytes(&self) -> &[u8] {
		&self.public
	}

	fn algorithm(&self) -> &'static SignatureAlgorithm {
		&PKCS_ECDSA_P256_SHA256
	}
}

impl rcgen::SigningKey for P256Key<'_> {
	fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rcgen::Error> {
		let signature: Signature = self.key.sign(message);
		Ok(signature.to_der().as_bytes().to_vec())
	}
}

#[cfg(test)]
mod tests {
	use std::net::Ipv4Addr;

	use rustls::pki_types::ServerName;
	use rustls::{ClientConfig, RootCertStore};
	use tokio::io::{AsyncReadExt, AsyncWriteExt};
	use tokio_rustls::{TlsAcceptor, TlsConnector};

	use super::{Identity, provider};

	const HOME: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 5);

	fn first_certificate(identity: &Identity) -> Vec<u8> {
		identity.certificates[0].to_vec()
	}

	#[test]
	fn the_certificate_is_kept_until_the_addresses_change() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let dir = directory.path().join(".server");
		let first = Identity::load_or_create(&dir, &[Ipv4Addr::LOCALHOST, HOME]).expect("a new certificate");
		let again = Identity::load_or_create(&dir, &[HOME, Ipv4Addr::LOCALHOST]).expect("the kept certificate");
		assert_eq!(
			first_certificate(&first),
			first_certificate(&again),
			"the same addresses keep the certificate"
		);
		assert!(
			first_certificate(&first)
				.windows(4)
				.any(|part| part == [192, 168, 1, 5]),
			"the certificate names the address"
		);
		let moved = Identity::load_or_create(&dir, &[Ipv4Addr::LOCALHOST, Ipv4Addr::new(10, 0, 0, 7)])
			.expect("a new certificate");
		assert_ne!(
			first_certificate(&first),
			first_certificate(&moved),
			"other addresses make a new one"
		);
		let kept = Identity::load_or_create(&dir, &[Ipv4Addr::LOCALHOST, Ipv4Addr::new(10, 0, 0, 7)])
			.expect("the kept certificate");
		assert_eq!(
			first_certificate(&moved),
			first_certificate(&kept),
			"the new certificate is kept in turn"
		);
	}

	#[tokio::test]
	async fn a_client_that_trusts_the_certificate_completes_a_handshake_with_the_provider() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let identity = Identity::load_or_create(directory.path(), &[HOME]).expect("a certificate");
		let mut roots = RootCertStore::empty();
		roots
			.add(identity.certificates[0].clone())
			.expect("the certificate is a trust anchor");
		let acceptor = TlsAcceptor::from(std::sync::Arc::new(identity.server_config().expect("a server config")));
		let client = ClientConfig::builder_with_provider(provider())
			.with_safe_default_protocol_versions()
			.expect("protocol versions")
			.with_root_certificates(roots)
			.with_no_client_auth();
		let (near, far) = tokio::io::duplex(1 << 16);
		let server = tokio::spawn(async move {
			let mut stream = acceptor.accept(far).await.expect("the server handshake");
			stream.write_all(b"hi").await.expect("a write");
			stream.shutdown().await.expect("a shutdown");
		});
		let name = ServerName::try_from("192.168.1.5").expect("an address");
		let mut stream = TlsConnector::from(std::sync::Arc::new(client))
			.connect(name, near)
			.await
			.expect("the client handshake");
		let mut text = String::new();
		stream.read_to_string(&mut text).await.expect("a read");
		assert_eq!(text, "hi");
		server.await.expect("the server task");
	}
}
