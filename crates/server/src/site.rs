//! The web site of the server: the game files, the room list, and the door of the bus.

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::header::{CACHE_CONTROL, CONNECTION, CONTENT_TYPE, UPGRADE};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use sim::room::{Join, encode_room_list};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;

use crate::bus::{Bus, socket_config};
use crate::files;

/// How long a new connection has to finish its TLS handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a client has to send the head of a request. It does not apply to a WebSocket, which
/// the bus watches with pings.
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
/// The most connections that the server serves at once: 64 rooms of 16 players, and some more for
/// the pages that load while the players play.
const MAX_CONNECTIONS: usize = 1200;

/// The answers of the server: the files of `web`, `/rooms`, and `/ws` for the bus.
#[derive(Clone, Debug)]
pub struct Site {
	web: Arc<PathBuf>,
	bus: Bus,
	max_connections: usize,
}

impl Site {
	/// Return a site that serves the directory `web` and the rooms of `bus`.
	#[must_use]
	pub fn new(web: PathBuf, bus: Bus) -> Self {
		Self {
			web: Arc::new(web),
			bus,
			max_connections: MAX_CONNECTIONS,
		}
	}

	/// Return this site, with a limit of `max_connections` connections that it serves at once.
	#[must_use]
	pub fn with_max_connections(self, max_connections: usize) -> Self {
		Self {
			max_connections,
			..self
		}
	}

	/// Answer one request. A valid request for `/ws` gets 101, and its connection then joins the bus.
	pub async fn respond<B>(&self, mut request: Request<B>) -> Response<Full<Bytes>> {
		if request.method() != Method::GET {
			return text(StatusCode::METHOD_NOT_ALLOWED, "only GET is allowed");
		}
		match request.uri().path() {
			"/rooms" => Response::builder()
				.header(CONTENT_TYPE, "application/json")
				.header(CACHE_CONTROL, "no-store")
				.body(Full::new(Bytes::from(encode_room_list(&self.bus.rooms()))))
				.expect("the headers are valid"),
			"/ws" => self.upgrade(&mut request),
			path => files::serve(&self.web, path).await,
		}
	}

	/// Answer a request for the bus with the handshake of a WebSocket, and hand the connection to the bus.
	fn upgrade<B>(&self, request: &mut Request<B>) -> Response<Full<Bytes>> {
		let headers = request.headers();
		let has = |name, word: &str| {
			headers
				.get_all(name)
				.iter()
				.filter_map(|value| value.to_str().ok())
				.any(|value| value.split(',').any(|part| part.trim().eq_ignore_ascii_case(word)))
		};
		let version_13 = headers
			.get("sec-websocket-version")
			.is_some_and(|version| version == "13");
		let Some(key) = headers
			.get("sec-websocket-key")
			.filter(|_| has(CONNECTION, "upgrade") && has(UPGRADE, "websocket") && version_13)
		else {
			return text(StatusCode::BAD_REQUEST, "this address takes a WebSocket");
		};
		let accept = derive_accept_key(key.as_bytes());
		let join = Join::parse_query(request.uri().query());
		let upgraded = hyper::upgrade::on(request);
		let bus = self.bus.clone();
		tokio::spawn(async move {
			if let Ok(upgraded) = upgraded.await {
				let ws =
					WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Server, Some(socket_config())).await;
				bus.connect(ws, join).await;
			}
		});
		Response::builder()
			.status(StatusCode::SWITCHING_PROTOCOLS)
			.header(CONNECTION, "upgrade")
			.header(UPGRADE, "websocket")
			.header("sec-websocket-accept", accept)
			.body(Full::default())
			.expect("the headers are valid")
	}

	/// Serve one connection with HTTP/1.1 until it ends.
	///
	/// # Errors
	///
	/// Returns the error of the connection, such as a request that is not valid HTTP.
	pub async fn serve_connection<S>(&self, stream: S) -> hyper::Result<()>
	where
		S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
	{
		let site = self.clone();
		let service = service_fn(move |request: Request<hyper::body::Incoming>| {
			let site = site.clone();
			async move { Ok::<_, Infallible>(site.respond(request).await) }
		});
		// Without a timer, hyper turns the timeout of the request head off.
		hyper::server::conn::http1::Builder::new()
			.timer(TokioTimer::new())
			.header_read_timeout(HEADER_TIMEOUT)
			.serve_connection(TokioIo::new(stream), service)
			.with_upgrades()
			.await
	}

	/// Accept connections for ever, and serve each over TLS.
	///
	/// It accepts no connection while the limit of open connections is reached. It logs each connection and each failed handshake in one line.
	pub async fn listen(self, listener: TcpListener, acceptor: TlsAcceptor) -> ! {
		let open = Arc::new(Semaphore::new(self.max_connections));
		loop {
			let permit = Arc::clone(&open)
				.acquire_owned()
				.await
				.expect("the semaphore is never closed");
			let (tcp, peer) = match listener.accept().await {
				Ok(accepted) => accepted,
				Err(error) => {
					eprintln!("cannot accept a connection: {error}");
					tokio::time::sleep(Duration::from_millis(100)).await;
					continue;
				}
			};
			eprintln!("{peer} connected");
			// Each message is small and the game cannot wait for Nagle's algorithm to join it to the next.
			if let Err(error) = tcp.set_nodelay(true) {
				eprintln!("{peer} keeps Nagle's algorithm: {error}");
			}
			let (site, acceptor) = (self.clone(), acceptor.clone());
			tokio::spawn(async move {
				let _permit = permit;
				match timeout(HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await {
					Ok(Ok(stream)) => {
						let _ = site.serve_connection(stream).await;
					}
					Ok(Err(error)) => eprintln!("{peer} failed the TLS handshake: {error}"),
					Err(_) => eprintln!("{peer} failed the TLS handshake: it took more than {HANDSHAKE_TIMEOUT:?}"),
				}
			});
		}
	}
}

fn text(status: StatusCode, body: &'static str) -> Response<Full<Bytes>> {
	Response::builder()
		.status(status)
		.header(CONTENT_TYPE, "text/plain; charset=utf-8")
		.body(Full::new(Bytes::from_static(body.as_bytes())))
		.expect("the headers are valid")
}

#[cfg(test)]
mod tests {
	use std::net::Ipv4Addr;
	use std::sync::Arc;
	use std::time::Duration;

	use futures_util::StreamExt;
	use http_body_util::{BodyExt, Empty};
	use hyper::header::{CACHE_CONTROL, CONTENT_TYPE};
	use hyper::{Request, StatusCode};
	use rustls::pki_types::pem::PemObject;
	use rustls::pki_types::{CertificateDer, ServerName};
	use rustls::{ClientConfig, RootCertStore};
	use sim::room::{RoomStatus, parse_room_list};
	use tokio::io::AsyncWriteExt;
	use tokio::net::{TcpListener, TcpStream};
	use tokio::time::timeout;
	use tokio_rustls::{TlsAcceptor, TlsConnector};
	use tokio_tungstenite::client_async;
	use tokio_tungstenite::tungstenite::Message;

	use super::{HEADER_TIMEOUT, Site};
	use crate::bus::Bus;
	use crate::tls::{Identity, provider};

	fn site() -> Site {
		Site::new("web".into(), Bus::new())
	}

	#[tokio::test]
	async fn the_room_list_is_json_that_nobody_may_keep() {
		let request = Request::get("/rooms")
			.body(Empty::<bytes::Bytes>::new())
			.expect("a request");
		let response = site().respond(request).await;
		assert_eq!(response.status(), StatusCode::OK);
		assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
		assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
		let body = response.into_body().collect().await.expect("a body").to_bytes();
		let rooms = parse_room_list(std::str::from_utf8(&body).expect("text")).expect("a room list");
		assert_eq!((rooms[0].id.as_str(), rooms[0].title.as_str()), ("0", "Open room"));
	}

	#[tokio::test]
	async fn a_request_for_the_bus_without_a_websocket_handshake_is_refused() {
		let request = Request::get("/ws")
			.body(Empty::<bytes::Bytes>::new())
			.expect("a request");
		assert_eq!(site().respond(request).await.status(), StatusCode::BAD_REQUEST);
	}

	#[tokio::test]
	async fn a_websocket_handshake_over_http_joins_the_room_that_the_query_names() {
		let site = site();
		let (near, far) = tokio::io::duplex(1 << 16);
		tokio::spawn({
			let site = site.clone();
			async move { site.serve_connection(far).await }
		});
		let (mut ws, response) = client_async("ws://host/ws?create=Friday", near)
			.await
			.expect("the handshake");
		assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
		let message = timeout(Duration::from_secs(5), ws.next())
			.await
			.expect("a frame")
			.expect("open")
			.expect("valid");
		let Message::Text(text) = message else {
			panic!("expected the status, got {message:?}")
		};
		let status = RoomStatus::parse_frame(&text).expect("a room status");
		assert_eq!((status.id.as_str(), status.title.as_str()), ("1", "Friday"));
	}

	#[tokio::test(start_paused = true)]
	async fn a_connection_that_never_finishes_its_request_head_is_closed() {
		let (mut near, far) = tokio::io::duplex(1 << 16);
		let site = site();
		let connection = tokio::spawn(async move { site.serve_connection(far).await });
		near.write_all(b"GET / HTTP/1.1\r\nHost: lan\r\n")
			.await
			.expect("a partial head");
		let ended = timeout(HEADER_TIMEOUT * 3, connection)
			.await
			.expect("the server must give up on a head that never ends")
			.expect("the connection task");
		assert!(ended.is_err(), "the connection ends with the error of the timeout");
	}

	#[tokio::test]
	async fn the_server_serves_no_more_connections_than_its_limit() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let identity = Identity::load_or_create(directory.path(), &[Ipv4Addr::LOCALHOST]).expect("a certificate");
		let mut roots = RootCertStore::empty();
		for certificate in
			CertificateDer::pem_file_iter(directory.path().join("cert.pem")).expect("the kept certificate")
		{
			roots.add(certificate.expect("a certificate")).expect("a trust anchor");
		}
		let acceptor = TlsAcceptor::from(Arc::new(identity.server_config().expect("a server config")));
		let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("a free port");
		let address = listener.local_addr().expect("an address");
		tokio::spawn(site().with_max_connections(1).listen(listener, acceptor));
		let connector = TlsConnector::from(Arc::new(
			ClientConfig::builder_with_provider(provider())
				.with_safe_default_protocol_versions()
				.expect("protocol versions")
				.with_root_certificates(roots)
				.with_no_client_auth(),
		));
		let name = ServerName::try_from("127.0.0.1").expect("an address");
		let first = connector
			.connect(
				name.clone(),
				TcpStream::connect(address).await.expect("the first connection"),
			)
			.await
			.expect("the first handshake");
		let second = TcpStream::connect(address).await.expect("the second connection");
		let waiting = connector.connect(name, second);
		tokio::pin!(waiting);
		assert!(
			timeout(Duration::from_millis(300), &mut waiting).await.is_err(),
			"the second connection must wait while the first holds the only place"
		);
		drop(first);
		timeout(Duration::from_secs(5), waiting)
			.await
			.expect("the second connection must get the place that the first left")
			.expect("the second handshake");
	}
}
