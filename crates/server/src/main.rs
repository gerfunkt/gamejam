//! The LAN server: HTTPS, the game files, and the message bus, in one
//! Rust program on one port. It knows nothing about the game.

mod bus;
mod cert;
mod load;

use bus::{Bus, Frame, Join};
use futures_util::{SinkExt, StreamExt};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response, StatusCode};
use hyper_tungstenite::{is_upgrade_request, tungstenite, HyperWebsocket};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;

const DEFAULT_PORT: u16 = 5201;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match args.get(1).map(String::as_str) {
        None | Some("serve") => {
            let port = args
                .get(2)
                .and_then(|p| p.parse().ok())
                .unwrap_or(DEFAULT_PORT);
            serve(port).await
        }
        Some("load") => {
            let Some(addr) = args.get(2) else {
                eprintln!("usage: server load <host:port> <clients> <seconds>");
                std::process::exit(2);
            };
            let clients = args.get(3).and_then(|n| n.parse().ok()).unwrap_or(16);
            let seconds = args.get(4).and_then(|n| n.parse().ok()).unwrap_or(10);
            load::run(addr, clients, seconds).await
        }
        Some(other) => {
            eprintln!("unknown command {other:?}; usage: server [serve [port]] | load <addr> <clients> <seconds>");
            2
        }
    };
    std::process::exit(code);
}

async fn serve(port: u16) -> i32 {
    let parts = match cert::load_or_create(Path::new(".server")) {
        Ok(parts) => parts,
        Err(e) => {
            eprintln!("certificate: {e}");
            return 1;
        }
    };
    let provider = rustls_rustcrypto::provider();
    let tls = match rustls::ServerConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
    {
        Ok(builder) => builder,
        Err(e) => {
            eprintln!("tls versions: {e}");
            return 1;
        }
    }
    .with_no_client_auth()
    .with_single_cert(parts.chain, parts.key)
    .map(Arc::new);
    let tls = match tls {
        Ok(tls) => tls,
        Err(e) => {
            eprintln!("tls config: {e}");
            return 1;
        }
    };
    let acceptor = TlsAcceptor::from(tls);
    let bus = Arc::new(Mutex::new(Bus::new()));
    let web = Arc::new(PathBuf::from("web"));

    let listener = match TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bind port {port}: {e}");
            return 1;
        }
    };
    println!("serving on:");
    println!("  https://localhost:{port}");
    for san in cert::san_list() {
        if let rcgen::SanType::IpAddress(ip) = san {
            println!("  https://{ip}:{port}");
        }
    }

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(_) => continue,
        };
        let acceptor = acceptor.clone();
        let bus = bus.clone();
        let web = web.clone();
        tokio::spawn(async move {
            let Ok(tls_stream) = acceptor.accept(stream).await else {
                return;
            };
            let io = TokioIo::new(tls_stream);
            let service = hyper::service::service_fn(move |req| {
                handle(req, bus.clone(), web.clone())
            });
            let conn = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .with_upgrades();
            let _ = conn.await;
        });
    }
}

async fn handle(
    mut req: Request<Incoming>,
    bus: Arc<Mutex<Bus>>,
    web: Arc<PathBuf>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();

    if path == "/rooms" {
        let list = bus.lock().unwrap().list();
        let mut body = String::from("[");
        for (i, room) in list.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push_str(&format!(
                "{{\"id\":\"{}\",\"title\":{},\"members\":{},\"owner\":{},\"info\":{}}}",
                room.id,
                json(&room.title),
                room.members,
                room.owner.map(|n| n.to_string()).unwrap_or_else(|| "null".into()),
                room.info.as_deref().map(json).unwrap_or_else(|| "null".into()),
            ));
        }
        body.push(']');
        return Ok(Response::builder()
            .header("Content-Type", "application/json")
            .header("Cache-Control", "no-store")
            .body(Full::new(Bytes::from(body)))
            .unwrap());
    }

    if path == "/ws" && is_upgrade_request(&req) {
        // Decide the join before the upgrade, so a refusal is a close
        // frame on a fresh socket, as the protocol says.
        let want = parse_join(&query);
        let ticket = {
            let mut bus = bus.lock().unwrap();
            match want {
                Ok(want) => bus.join(want).map(|(num, outs)| (num, outs)),
                Err(e) => Err(e),
            }
        };
        let (response, websocket) = match hyper_tungstenite::upgrade(&mut req, None) {
            Ok(pair) => pair,
            Err(e) => {
                return Ok(text(StatusCode::BAD_REQUEST, &format!("upgrade: {e}")));
            }
        };
        tokio::spawn(serve_ws(websocket, bus, ticket));
        return Ok(response);
    }

    Ok(serve_file(&web, &path))
}

/// The join request from the query: nothing, `room=<id>`, or
/// `create=<title>`.
fn parse_join(query: &str) -> Result<Join, (u16, &'static str)> {
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "room" => {
                let id = value.parse().map_err(|_| {
                    (bus::close::NO_SUCH_ROOM, "no such room")
                })?;
                return Ok(Join::Existing(id));
            }
            "create" => return Ok(Join::Create(percent_decode(value))),
            _ => {}
        }
    }
    Ok(Join::Open)
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn serve_ws(
    websocket: HyperWebsocket,
    bus: Arc<Mutex<Bus>>,
    ticket: Result<(u32, Vec<bus::Out>), (u16, &'static str)>,
) {
    let Ok(ws) = websocket.await else {
        return;
    };
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Frame>();

    let (num, join_outs) = match ticket {
        Ok(pair) => pair,
        Err((code, reason)) => {
            let _ = sink
                .send(tungstenite::Message::Close(Some(tungstenite::protocol::CloseFrame {
                    code: code.into(),
                    reason: reason.into(),
                })))
                .await;
            return;
        }
    };

    // The writer: one task owns the sink, fed by the bus.
    let writer = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            let message = match frame {
                Frame::Binary(bytes) => tungstenite::Message::Binary(bytes.into()),
                Frame::Text(text) => tungstenite::Message::Text(text.into()),
                Frame::Close(code, reason) => {
                    let _ = sink
                        .send(tungstenite::Message::Close(Some(
                            tungstenite::protocol::CloseFrame {
                                code: code.into(),
                                reason: reason.into(),
                            },
                        )))
                        .await;
                    break;
                }
            };
            if sink.send(message).await.is_err() {
                break;
            }
        }
    });

    // Register, then deliver the join status frames.
    registry().lock().unwrap().insert(num, tx.clone());
    fan_out(&tx, join_outs);

    while let Some(message) = stream.next().await {
        let Ok(message) = message else { break };
        let outs = {
            let mut bus = bus.lock().unwrap();
            match message {
                tungstenite::Message::Binary(bytes) => bus.binary(num, bytes.as_slice()),
                tungstenite::Message::Text(text) => bus.text(num, text.as_str()),
                tungstenite::Message::Close(_) => break,
                _ => continue,
            }
        };
        fan_out(&tx, outs);
    }

    let outs = bus.lock().unwrap().leave(num);
    registry().lock().unwrap().remove(&num);
    fan_out(&tx, outs);
    drop(tx);
    let _ = writer.await;
}

/// Members hold their sender in a registry keyed by player number.
fn registry() -> &'static Mutex<std::collections::HashMap<u32, mpsc::UnboundedSender<Frame>>> {
    static REGISTRY: std::sync::OnceLock<
        Mutex<std::collections::HashMap<u32, mpsc::UnboundedSender<Frame>>>,
    > = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn fan_out(own: &mpsc::UnboundedSender<Frame>, outs: Vec<bus::Out>) {
    let registry = registry().lock().unwrap();
    for out in outs {
        let tx = registry.get(&out.to).unwrap_or(own);
        let _ = tx.send(out.frame);
    }
}

fn serve_file(web: &Path, path: &str) -> Response<Full<Bytes>> {
    let path = if path == "/" { "/index.html" } else { path };
    let mut file = web.to_path_buf();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => file.push(part),
            Component::RootDir => {}
            _ => return text(StatusCode::FORBIDDEN, "forbidden"),
        }
    }
    match std::fs::read(&file) {
        Ok(bytes) => {
            let mime = match file.extension().and_then(|e| e.to_str()) {
                Some("html") => "text/html; charset=utf-8",
                Some("js") => "text/javascript",
                Some("wasm") => "application/wasm",
                Some("css") => "text/css",
                Some("png") => "image/png",
                Some("json") => "application/json",
                Some("wav") => "audio/wav",
                Some("mp3") => "audio/mpeg",
                Some("ogg") => "audio/ogg",
                _ => "application/octet-stream",
            };
            Response::builder()
                .header("Content-Type", mime)
                .header("Cache-Control", "no-store")
                .body(Full::new(Bytes::from(bytes)))
                .unwrap()
        }
        Err(_) => text(StatusCode::NOT_FOUND, "not found"),
    }
}

fn json(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn text(status: StatusCode, body: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap()
}
