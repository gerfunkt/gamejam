//! A load test for the bus: N clients, each sending 60 messages a
//! second; the bus echoes to every member, and each client measures the
//! round trip of its own messages.

use futures_util::{SinkExt, StreamExt};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

const RATE_HZ: f64 = 60.0;

#[derive(Debug)]
struct AcceptAll;

impl ServerCertVerifier for AcceptAll {
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
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ED25519,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PKCS1_SHA256,
        ]
    }
}

fn danger_config() -> ClientConfig {
    let provider = rustls_rustcrypto::provider();
    ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .expect("tls versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAll))
        .with_no_client_auth()
}

/// One client. Returns the round trips of its own echoed messages.
async fn client(url: &str, tag: u32, seconds: u64, start: Instant) -> Vec<f64> {
    let connector = tokio_tungstenite::Connector::Rustls(Arc::new(danger_config()));
    let Ok((ws, _)) =
        tokio_tungstenite::connect_async_tls_with_config(url, None, false, Some(connector)).await
    else {
        eprintln!("client {tag}: connect failed");
        return Vec::new();
    };
    let (mut sink, mut stream) = ws.split();

    let sender = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs_f64(1.0 / RATE_HZ));
        let mut n = 0u64;
        let end = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < end {
            interval.tick().await;
            n += 1;
            let mut payload = Vec::with_capacity(20);
            payload.extend_from_slice(&tag.to_le_bytes());
            payload.extend_from_slice(&n.to_le_bytes());
            payload.extend_from_slice(&start.elapsed().as_secs_f64().to_le_bytes());
            if sink.send(Message::Binary(payload.into())).await.is_err() {
                break;
            }
        }
    });

    let mut rtts = Vec::new();
    let grace = Duration::from_secs(seconds) + Duration::from_secs(2);
    while let Ok(Some(message)) = tokio::time::timeout(grace, stream.next()).await {
        let Ok(Message::Binary(bytes)) = message else { continue };
        // The bus prefixes the sender number; the payload holds the
        // client tag, the sequence, and the send time.
        if bytes.len() != 4 + 20 || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != tag {
            continue;
        }
        let sent = f64::from_le_bytes(bytes[16..24].try_into().unwrap());
        rtts.push(start.elapsed().as_secs_f64() - sent);
    }
    sender.abort();
    rtts
}

pub async fn run(addr: &str, clients: usize, seconds: u64) -> i32 {
    let url = format!("wss://{addr}/ws");
    let start = Instant::now();
    let mut handles = Vec::new();
    for tag in 0..clients as u32 {
        let url = url.clone();
        handles.push(tokio::spawn(async move { client(&url, tag, seconds, start).await }));
    }
    let mut samples = Vec::new();
    for handle in handles {
        if let Ok(rtts) = handle.await {
            samples.extend(rtts);
        }
    }
    if samples.is_empty() {
        eprintln!("no samples; is the server running at {addr}?");
        return 1;
    }
    samples.sort_by(|a, b| a.total_cmp(b));
    let pick = |p: f64| samples[((samples.len() - 1) as f64 * p) as usize];
    println!("{} round trips in {seconds} s over {clients} clients:", samples.len());
    println!("  median: {:.2} ms", pick(0.5) * 1000.0);
    println!("  95th:   {:.2} ms", pick(0.95) * 1000.0);
    println!("  99th:   {:.2} ms", pick(0.99) * 1000.0);
    println!("  worst:  {:.2} ms", samples.last().unwrap() * 1000.0);
    0
}
