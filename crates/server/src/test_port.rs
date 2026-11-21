//! A free port for a test. Requirement N9 keeps every test port between 5201 and 5299.

use std::net::Ipv4Addr;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::net::TcpListener;

const FIRST: u16 = 5201;
const COUNT: u16 = 99;

/// Bind a listener on loopback to the first free port of the range, from a start that varies
/// with the process and the time, so that parallel tests and other programs do not clash.
pub async fn bind() -> TcpListener {
	let clock = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0, |elapsed| elapsed.subsec_nanos());
	let start = u16::try_from((clock ^ std::process::id().wrapping_mul(2_654_435_761)) % u32::from(COUNT))
		.expect("a remainder of 99 fits in a u16");
	for step in 0..COUNT {
		let port = FIRST + (start + step) % COUNT;
		if let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
			return listener;
		}
	}
	panic!("no port from {FIRST} to {} is free", FIRST + COUNT - 1);
}
