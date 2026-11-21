//! The load test of the bus: many clients that send and hear their own messages.
//!
//! The tool connects over TLS and accepts any certificate, because it tests a server that
//! it does not trust or distrust. No other part of the crate does this.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use sim::room::{MAX_MEMBERS, MAX_ROOMS, RoomStatus, split_frame};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{WebSocketStream, client_async};

use crate::tls::provider;

/// How many messages each client sends in a second.
const RATE: u32 = 60;
/// The size of each message: eight bytes of send time, and padding to the size of a plane state.
const MESSAGE_BYTES: usize = 40;
/// How long the clients wait for the last messages after the end of the sending.
const TAIL: Duration = Duration::from_millis(500);

/// The delay from a send to the receipt of the same message by its sender.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
	/// The number of messages that came back.
	pub messages: usize,
	/// The median delay.
	pub median: Duration,
	/// The delay that 99 of 100 messages stayed under.
	pub p99: Duration,
}

impl std::fmt::Display for Report {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let ms = |delay: Duration| delay.as_secs_f64() * 1000.0;
		write!(
			formatter,
			"{} messages: median {:.2} ms, 99th percentile {:.2} ms",
			self.messages,
			ms(self.median),
			ms(self.p99)
		)
	}
}

/// Run the load test against the server at `target` (`host:port`).
///
/// It connects `clients` clients, with at most 16 in each room, and lets each send 60 messages
/// a second for `seconds` seconds.
///
/// # Errors
///
/// Returns an error when `clients` is zero, when the rooms would exceed the limit of the
/// server, or when a connection fails.
pub async fn run(target: &str, clients: usize, seconds: u64) -> Result<Report> {
	if clients == 0 {
		bail!("the test needs at least one client");
	}
	if clients > (MAX_ROOMS - 1) * MAX_MEMBERS {
		bail!(
			"the server holds at most {} clients in rooms that this test creates",
			(MAX_ROOMS - 1) * MAX_MEMBERS
		);
	}
	let connector = TlsConnector::from(Arc::new(client_config()?));
	let host = target.rsplit_once(':').map_or(target, |(host, _)| host);
	let name = ServerName::try_from(host.to_owned()).with_context(|| format!("{host} is not a host name"))?;
	let start = Instant::now();
	let mut tasks: Vec<JoinHandle<Vec<Duration>>> = Vec::new();
	let mut room = String::new();
	for index in 0..clients {
		// The first client of each group makes a room. The others join it.
		let query = if index % MAX_MEMBERS == 0 {
			"?create=Load".to_owned()
		} else {
			format!("?room={room}")
		};
		let tcp = TcpStream::connect(target)
			.await
			.with_context(|| format!("cannot connect to {target}"))?;
		tcp.set_nodelay(true).context("cannot turn off Nagle's algorithm")?;
		let tls = connector
			.connect(name.clone(), tcp)
			.await
			.context("the TLS handshake failed")?;
		let (mut ws, _) = client_async(format!("wss://{target}/ws{query}"), tls)
			.await
			.context("the WebSocket handshake failed")?;
		let status = first_status(&mut ws).await?;
		room = status.id;
		tasks.push(tokio::spawn(client(
			ws,
			status.you,
			start,
			Duration::from_secs(seconds),
		)));
	}
	let mut delays = Vec::new();
	for task in tasks {
		delays.extend(task.await.context("a client task failed")?);
	}
	delays.sort_unstable();
	let Some(median) = delays.get(delays.len() / 2).copied() else {
		bail!("no message came back");
	};
	let p99 = delays[(delays.len() * 99 / 100).min(delays.len() - 1)];
	Ok(Report {
		messages: delays.len(),
		median,
		p99,
	})
}

async fn first_status<S>(ws: &mut WebSocketStream<S>) -> Result<RoomStatus>
where
	S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
	loop {
		match ws.next().await {
			Some(Ok(Message::Text(text))) => {
				return RoomStatus::parse_frame(&text).context("the server sent text that is no room status");
			}
			Some(Ok(Message::Close(frame))) => bail!("the server refused the join: {frame:?}"),
			Some(Ok(_)) => {}
			Some(Err(error)) => return Err(error).context("the connection failed"),
			None => bail!("the server closed the connection"),
		}
	}
}

/// Send for `duration`, and return the delay of each message that came back.
async fn client<S>(ws: WebSocketStream<S>, you: u32, start: Instant, duration: Duration) -> Vec<Duration>
where
	S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
	let (mut sink, mut stream) = ws.split();
	let sender = tokio::spawn(async move {
		let mut ticks = tokio::time::interval(Duration::from_secs(1) / RATE);
		ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
		let end = Instant::now() + duration;
		while Instant::now() < end {
			ticks.tick().await;
			let mut message = vec![0_u8; MESSAGE_BYTES];
			let sent = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
			message[..8].copy_from_slice(&sent.to_le_bytes());
			if sink.send(Message::Binary(message.into())).await.is_err() {
				break;
			}
		}
		sink
	});
	let mut delays = Vec::new();
	let mut sender = Some(sender);
	let mut deadline = None;
	loop {
		let wait = deadline.map_or(Duration::from_secs(3600), |end: Instant| {
			end.saturating_duration_since(Instant::now())
		});
		let Ok(Some(Ok(message))) = tokio::time::timeout(wait, stream.next()).await else {
			break;
		};
		if let Message::Binary(frame) = message
			&& let Some((from, body)) = split_frame(&frame)
			&& from == you
			&& let Ok(sent) = <[u8; 8]>::try_from(&body[..8.min(body.len())])
		{
			let now = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
			delays.push(Duration::from_nanos(now.saturating_sub(u64::from_le_bytes(sent))));
		}
		if deadline.is_none() && sender.as_ref().is_some_and(JoinHandle::is_finished) {
			deadline = Some(Instant::now() + TAIL);
		}
	}
	if let Some(sender) = sender.take() {
		sender.abort();
	}
	delays
}

fn client_config() -> Result<ClientConfig> {
	let provider = provider();
	Ok(ClientConfig::builder_with_provider(Arc::clone(&provider))
		.with_safe_default_protocol_versions()
		.context("the provider supports no protocol version")?
		.dangerous()
		.with_custom_certificate_verifier(Arc::new(AcceptAny(provider)))
		.with_no_client_auth())
}

/// A verifier that accepts every certificate and every signature.
#[derive(Debug)]
struct AcceptAny(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAny {
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
		_certificate: &CertificateDer<'_>,
		_signature: &DigitallySignedStruct,
	) -> Result<HandshakeSignatureValid, rustls::Error> {
		Ok(HandshakeSignatureValid::assertion())
	}

	fn verify_tls13_signature(
		&self,
		_message: &[u8],
		_certificate: &CertificateDer<'_>,
		_signature: &DigitallySignedStruct,
	) -> Result<HandshakeSignatureValid, rustls::Error> {
		Ok(HandshakeSignatureValid::assertion())
	}

	fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
		self.0.signature_verification_algorithms.supported_schemes()
	}
}

#[cfg(test)]
mod tests {
	use std::net::Ipv4Addr;
	use std::sync::Arc;

	use tokio_rustls::TlsAcceptor;

	use crate::bus::Bus;
	use crate::site::Site;
	use crate::tls::Identity;

	#[tokio::test]
	async fn the_load_test_measures_the_delay_against_a_running_server() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let identity = Identity::load_or_create(directory.path(), &[Ipv4Addr::LOCALHOST]).expect("a certificate");
		let acceptor = TlsAcceptor::from(Arc::new(identity.server_config().expect("a server config")));
		let listener = crate::test_port::bind().await;
		let target = listener.local_addr().expect("an address").to_string();
		tokio::spawn(Site::new("web".into(), Bus::new()).listen(listener, acceptor));
		let report = super::run(&target, 17, 1).await.expect("the load test");
		assert!(
			report.messages > 17 * 40,
			"most of the sent messages came back: {report}"
		);
		assert!(report.median <= report.p99, "the percentiles are in order: {report}");
	}
}
