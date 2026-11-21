//! The LAN host of the game: HTTPS for the files, the bus at `/ws`, and its rooms at `/rooms`.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use server::bus::Bus;
use server::load;
use server::site::Site;
use server::tls::{Identity, machine_addresses};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

/// Where the server keeps the certificate that it makes.
const CERTIFICATE_DIRECTORY: &str = ".server";

#[derive(Parser)]
#[command(about = "Serve the game over HTTPS, with the bus at /ws")]
struct Cli {
	/// The port to listen on, on every address of the machine.
	#[arg(long, default_value_t = 5201)]
	port: u16,
	/// The directory of the game files.
	#[arg(long, default_value = "web")]
	web: PathBuf,
	/// A PEM certificate chain to use in place of the one that the server makes.
	#[arg(long, requires = "key")]
	cert: Option<PathBuf>,
	/// The PEM private key of `--cert`.
	#[arg(long, requires = "cert")]
	key: Option<PathBuf>,
	#[command(subcommand)]
	command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
	/// Test the bus of a running server, and print the median and the 99th percentile of the delay.
	Load {
		/// The address of the server, as `host:port`.
		target: String,
		/// The number of clients, with at most 16 in each room.
		clients: usize,
		/// The seconds that the clients send.
		seconds: u64,
	},
}

#[tokio::main]
async fn main() -> Result<()> {
	let cli = Cli::parse();
	if let Some(Command::Load {
		target,
		clients,
		seconds,
	}) = cli.command
	{
		println!("{}", load::run(&target, clients, seconds).await?);
		return Ok(());
	}
	let addresses = machine_addresses()?;
	let identity = match (&cli.cert, &cli.key) {
		(Some(cert), Some(key)) => Identity::load(cert, key)?,
		_ => Identity::load_or_create(&PathBuf::from(CERTIFICATE_DIRECTORY), &addresses)?,
	};
	let acceptor = TlsAcceptor::from(Arc::new(identity.server_config()?));
	let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, cli.port))
		.await
		.with_context(|| format!("cannot listen on port {}", cli.port))?;
	for address in &addresses {
		println!("https://{address}:{}", cli.port);
	}
	Site::new(cli.web, Bus::new()).listen(listener, acceptor).await
}
