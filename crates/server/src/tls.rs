//! The certificate of the server, and the TLS settings that use it.
//!
//! Everything here is pure Rust: `rcgen` writes the certificate, `p256` signs it, and
//! `rustls-rustcrypto` is the cryptography of `rustls`.

use std::fs;
use std::io::Write;
use std::net::Ipv4Addr;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::Generate;
use p256::pkcs8::LineEnding;
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey};
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
	/// `addresses`, which lists the addresses of the certificate. A new pair replaces the old one
	/// when the list differs, or when the key and the certificate do not load as a pair.
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
			// A key and a certificate that do not fit are a damaged pair, and a new pair replaces them.
			&& identity.fits()
			&& identity.server_config().is_ok()
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

	/// Return whether the key is a P-256 key whose public half is in the first certificate.
	///
	/// `rustls` does not check this for every provider, so the check looks for the public point
	/// of the key in the certificate.
	fn fits(&self) -> bool {
		let PrivateKeyDer::Pkcs8(der) = &self.key else {
			return false;
		};
		let Ok(key) = SigningKey::from_pkcs8_der(der.secret_pkcs8_der()) else {
			return false;
		};
		let point = key.verifying_key().to_sec1_point(false);
		self.certificates.first().is_some_and(|certificate| {
			certificate
				.windows(point.as_bytes().len())
				.any(|part| part == point.as_bytes())
		})
	}

	/// Return the TLS settings of a server that presents this certificate, over HTTP/1.1 only.
	///
	/// # Errors
	///
	/// Returns an error when the key does not belong to the certificate, or the provider cannot
	/// use it.
	pub fn server_config(&self) -> Result<ServerConfig> {
		let mut config = ServerConfig::builder_with_provider(provider())
			.with_safe_default_protocol_versions()
			.context("the provider supports no protocol version")?
			.with_no_client_auth()
			.with_single_cert(self.certificates.clone(), self.key.clone_key())
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

/// Write `content` to `path` through a temporary file and a rename, so that a reader sees the
/// old file or the new one, and never half of it. The file gets `mode`, also if it existed.
fn write(path: &Path, content: &[u8], mode: u32) -> Result<()> {
	let temporary = path.with_extension("tmp");
	fs::OpenOptions::new()
		.write(true)
		.create(true)
		.truncate(true)
		.mode(mode)
		.open(&temporary)
		.and_then(|mut file| file.write_all(content))
		.and_then(|()| fs::set_permissions(&temporary, fs::Permissions::from_mode(mode)))
		.and_then(|()| fs::rename(&temporary, path))
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
	use std::fs;
	use std::net::Ipv4Addr;
	use std::os::unix::fs::PermissionsExt;
	use std::path::Path;
	use std::sync::Arc;

	use rustls::pki_types::pem::PemObject;
	use rustls::pki_types::{CertificateDer, ServerName};
	use rustls::{ClientConfig, RootCertStore};
	use tokio::io::{AsyncReadExt, AsyncWriteExt};
	use tokio_rustls::{TlsAcceptor, TlsConnector};

	use super::{Identity, provider};

	const HOME: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 5);

	fn kept_certificate(dir: &Path) -> Vec<u8> {
		fs::read(dir.join("cert.pem")).expect("the kept certificate")
	}

	/// Return whether a client that trusts the kept certificate completes a handshake with `identity`
	/// for the address `name`.
	async fn handshake_works(identity: &Identity, dir: &Path, name: &'static str) -> bool {
		let mut roots = RootCertStore::empty();
		for certificate in CertificateDer::pem_file_iter(dir.join("cert.pem")).expect("the kept certificate") {
			roots.add(certificate.expect("a certificate")).expect("a trust anchor");
		}
		let acceptor = TlsAcceptor::from(Arc::new(identity.server_config().expect("a server config")));
		let client = ClientConfig::builder_with_provider(provider())
			.with_safe_default_protocol_versions()
			.expect("protocol versions")
			.with_root_certificates(roots)
			.with_no_client_auth();
		let (near, far) = tokio::io::duplex(1 << 16);
		let server = tokio::spawn(async move {
			let mut stream = acceptor.accept(far).await.ok()?;
			stream.write_all(b"hi").await.ok()?;
			stream.shutdown().await.ok()
		});
		let name = ServerName::try_from(name).expect("a name");
		let Ok(mut stream) = TlsConnector::from(Arc::new(client)).connect(name, near).await else {
			return false;
		};
		let mut text = String::new();
		let read = stream.read_to_string(&mut text).await;
		server.await.expect("the server task");
		read.is_ok() && text == "hi"
	}

	#[test]
	fn the_certificate_is_kept_until_the_addresses_change() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let dir = directory.path().join(".server");
		Identity::load_or_create(&dir, &[Ipv4Addr::LOCALHOST, HOME]).expect("a new certificate");
		let first = kept_certificate(&dir);
		Identity::load_or_create(&dir, &[HOME, Ipv4Addr::LOCALHOST]).expect("the kept certificate");
		assert_eq!(first, kept_certificate(&dir), "the same addresses keep the certificate");
		let moved = [Ipv4Addr::LOCALHOST, Ipv4Addr::new(10, 0, 0, 7)];
		Identity::load_or_create(&dir, &moved).expect("a new certificate");
		let second = kept_certificate(&dir);
		assert_ne!(first, second, "other addresses make a new one");
		Identity::load_or_create(&dir, &moved).expect("the kept certificate");
		assert_eq!(second, kept_certificate(&dir), "the new certificate is kept in turn");
	}

	#[tokio::test]
	async fn a_client_that_trusts_the_certificate_completes_a_handshake_with_the_provider() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let identity = Identity::load_or_create(directory.path(), &[HOME]).expect("a certificate");
		assert!(
			handshake_works(&identity, directory.path(), "192.168.1.5").await,
			"the address is named"
		);
		assert!(
			handshake_works(&identity, directory.path(), "localhost").await,
			"localhost is named"
		);
		assert!(
			!handshake_works(&identity, directory.path(), "10.0.0.9").await,
			"another address is not"
		);
	}

	#[tokio::test]
	async fn a_kept_key_that_does_not_fit_the_certificate_is_replaced() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let (dir, other) = (directory.path().join("a"), directory.path().join("b"));
		Identity::load_or_create(&dir, &[HOME]).expect("a certificate");
		Identity::load_or_create(&other, &[HOME]).expect("another certificate");
		let before = kept_certificate(&dir);
		fs::copy(other.join("key.pem"), dir.join("key.pem")).expect("a key of another pair");
		let identity = Identity::load_or_create(&dir, &[HOME]).expect("a new pair instead of an error");
		assert_ne!(before, kept_certificate(&dir), "the pair was made again");
		assert!(
			handshake_works(&identity, &dir, "192.168.1.5").await,
			"the new pair works"
		);
	}

	#[test]
	fn the_key_file_is_private_also_when_it_existed_and_no_temporary_file_stays() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		fs::write(directory.path().join("key.pem"), "old").expect("an old key file");
		fs::set_permissions(directory.path().join("key.pem"), fs::Permissions::from_mode(0o644)).expect("a mode");
		Identity::load_or_create(directory.path(), &[HOME]).expect("a certificate");
		let mode = fs::metadata(directory.path().join("key.pem"))
			.expect("the key file")
			.permissions()
			.mode();
		assert_eq!(mode & 0o777, 0o600, "only the owner reads the key");
		let mut names: Vec<_> = fs::read_dir(directory.path())
			.expect("the directory")
			.map(|entry| entry.expect("an entry").file_name().into_string().expect("a name"))
			.collect();
		names.sort();
		assert_eq!(names, ["addresses", "cert.pem", "key.pem"]);
	}
}
