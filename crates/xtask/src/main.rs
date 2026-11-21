use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        usage();
        std::process::exit(2);
    };
    let release = args.any(|arg| arg == "--release");
    let root = root();
    let result = match command.as_str() {
        "build" => build(&root, release),
        "serve" => build(&root, release).and_then(|_| serve(&root, release)),
        _ => {
            eprintln!("unknown command {command:?}");
            usage();
            Err(2)
        }
    };
    std::process::exit(result.err().unwrap_or(0));
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn usage() {
    eprintln!("usage: cargo xtask build [--release] | serve [--release]");
}

fn build(root: &Path, release: bool) -> Result<(), i32> {
    let mut cargo = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cargo
        .current_dir(root)
        .arg("build")
        .arg("-p")
        .arg("client")
        .arg("--target")
        .arg("wasm32-unknown-unknown");
    if release {
        cargo.arg("--release");
    }
    run(&mut cargo)?;

    let profile = if release { "release" } else { "debug" };
    let wasm = root
        .join("target/wasm32-unknown-unknown")
        .join(profile)
        .join("client.wasm");
    let web = root.join("web");
    std::fs::create_dir_all(&web).map_err(|error| {
        eprintln!("create {}: {error}", web.display());
        1
    })?;
    let mut bindgen = Command::new("wasm-bindgen");
    bindgen
        .current_dir(root)
        .arg("--target")
        .arg("web")
        .arg("--no-typescript")
        .arg("--out-name")
        .arg("app")
        .arg("--out-dir")
        .arg(&web)
        .arg(&wasm);
    run(&mut bindgen)
}

fn serve(root: &Path, release: bool) -> Result<(), i32> {
    let mut server = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    server
        .current_dir(root)
        .arg("run")
        .arg("-p")
        .arg("server")
        .arg("--")
        .arg("serve")
        .arg("5201");
    if release {
        server.arg("--release");
    }
    server.stdin(Stdio::inherit());
    run(&mut server)
}

fn run(command: &mut Command) -> Result<(), i32> {
    let status = command.status().map_err(|error| {
        eprintln!("start {:?}: {error}", command.get_program());
        1
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(status.code().unwrap_or(1))
    }
}
