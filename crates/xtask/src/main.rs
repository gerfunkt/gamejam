//! The build tasks of the workspace.

use std::{
	env,
	io::{BufRead, BufReader},
	path::{Path, PathBuf},
	process::{Command, Stdio},
	time::Instant,
};

use anyhow::{Context, Result, bail, ensure};

mod models;

const USAGE: &str = "\
cargo xtask serve [--dev] [--webgpu] [-- <server options>]
    Build the game, and serve it over HTTPS with the message bus. Open the printed address.
    The task builds the client for WebGPU and the client for WebGL2, which phones and older
    browsers need. --webgpu builds only the first, for a short loop of change and test.
    --dev builds without the long optimisation, and keeps the function names for crash reports.

cargo xtask build [--dev] [--webgpu]
    Build the clients for the browser into web/, as serve does. It converts the plane models first.

cargo xtask models [--preview]
    Turn the plane models of assets/planes/ into the GLB files of web/assets/planes/.
    --preview also draws each model into assets/planes/<key>-preview.png, from above and from the side.";

/// A build of the client for one graphics interface of the browser.
#[derive(Clone, Copy)]
enum Backend {
	/// The build for browsers with WebGPU.
	WebGpu,
	/// The build for browsers without WebGPU. It draws slower.
	WebGl2,
}

impl Backend {
	/// The directory of the build inside `web/`. The page loads the client from there.
	fn directory(self) -> &'static str {
		match self {
			Backend::WebGpu => "webgpu",
			Backend::WebGl2 => "webgl2",
		}
	}
}

fn main() -> Result<()> {
	let args: Vec<String> = env::args().skip(1).collect();
	// Everything after `--` goes to the server unchanged, such as `--port 5211`.
	let (own, server) = args.split_at(args.iter().position(|arg| arg == "--").unwrap_or(args.len()));
	let server = server.get(1..).unwrap_or_default();
	let (task, flags) = own
		.split_first()
		.map_or(("", &[][..]), |(task, flags)| (task.as_str(), flags));
	let known: &[&str] = if task == "models" {
		&["--preview"]
	} else {
		&["--dev", "--webgpu"]
	};
	ensure!(
		flags.iter().all(|flag| known.contains(&flag.as_str())),
		"unknown option\n\n{USAGE}"
	);
	let release = !flags.iter().any(|flag| flag == "--dev");
	let all = !flags.iter().any(|flag| flag == "--webgpu");
	match task {
		"build" => build(release, all),
		"models" => models::run(&root()?, flags.iter().any(|flag| flag == "--preview")),
		"serve" => {
			build(release, all)?;
			// The server always builds in release, because it carries the bus for every player.
			let mut run = Command::new(cargo());
			run.current_dir(root()?)
				.args(["run", "--release", "-p", "server", "--"])
				.args(server);
			check(&mut run)
		}
		_ => {
			println!("{USAGE}");
			Ok(())
		}
	}
}

fn root() -> Result<PathBuf> {
	Ok(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?)
}

/// Build the client into `web/`, for WebGPU, and for WebGL2 too if `all` is set.
fn build(release: bool, all: bool) -> Result<()> {
	let root = root()?;
	models::run(&root, false)?;
	for backend in [Backend::WebGpu, Backend::WebGl2]
		.into_iter()
		.take(if all { 2 } else { 1 })
	{
		let started = Instant::now();
		let wasm = compile(&root, backend, release)?;
		let mut bindgen = Command::new("wasm-bindgen");
		bindgen.args(["--target", "web", "--no-typescript", "--out-name", "app", "--out-dir"]);
		bindgen.arg(root.join("web").join(backend.directory()));
		if release {
			// The function names are about half of the file.
			bindgen.args(["--remove-name-section", "--remove-producers-section"]);
		}
		check(bindgen.arg(wasm)).context("is the wasm-bindgen tool installed? Run `mise install`")?;
		println!(
			"built web/{} in {:.1} s",
			backend.directory(),
			started.elapsed().as_secs_f64()
		);
	}
	Ok(())
}

/// Compile the client for the browser, and return the path of the .wasm file.
///
/// Mr. Boxington chooses the place of the build, so the path comes from the messages of cargo.
fn compile(root: &Path, backend: Backend, release: bool) -> Result<PathBuf> {
	let mut command = Command::new(cargo());
	command.current_dir(root);
	command.args([
		"build",
		"-p",
		"client",
		"--target",
		"wasm32-unknown-unknown",
		"--message-format=json-render-diagnostics",
	]);
	if release {
		command.arg("--release");
	}
	if matches!(backend, Backend::WebGl2) {
		// The client draws with WebGPU by default. Without that feature, the engine uses WebGL2.
		command.arg("--no-default-features");
	}
	let mut child = command.stdout(Stdio::piped()).spawn().context("cannot run cargo")?;
	let mut wasm = None;
	for line in BufReader::new(child.stdout.take().context("the output of cargo is not piped")?).lines() {
		let message: serde_json::Value = serde_json::from_str(&line?).unwrap_or_default();
		let mut files = message["filenames"]
			.as_array()
			.into_iter()
			.flatten()
			.filter_map(serde_json::Value::as_str);
		if let Some(file) = files.rfind(|file| file.ends_with("client.wasm")) {
			wasm = Some(PathBuf::from(file));
		}
	}
	ensure!(child.wait()?.success(), "the build of the client failed");
	wasm.context("the build named no .wasm file")
}

fn check(command: &mut Command) -> Result<()> {
	let status = command
		.status()
		.with_context(|| format!("cannot run {:?}", command.get_program()))?;
	if !status.success() {
		bail!("{:?} failed: {status}", command.get_program());
	}
	Ok(())
}

/// Return the build tool: Mr. Boxington, which shares compiled crates between checkouts, or else cargo.
fn cargo() -> String {
	let on_path = |program: &str| {
		env::var_os("PATH")
			.is_some_and(|path| env::split_paths(&path).any(|directory| directory.join(program).is_file()))
	};
	if on_path("mbx") {
		"mbx".into()
	} else {
		env::var("CARGO").unwrap_or_else(|_| "cargo".into())
	}
}
