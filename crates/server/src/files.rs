//! The files of the game, read from one directory.

use std::path::{Path, PathBuf};

use bytes::Bytes;
use http_body_util::Full;
use hyper::header::{CACHE_CONTROL, CONTENT_TYPE};
use hyper::{Response, StatusCode};
use percent_encoding::percent_decode_str;

/// Return the response for the request path `path`, with the file below `root` that it names.
///
/// A path that leaves `root`, by `..` or by a link, gets 403. A missing file gets 404. The
/// response asks the browser to check for a new version on each load, so that a new build of
/// the game loads after a reload.
pub async fn serve(root: &Path, path: &str) -> Response<Full<Bytes>> {
	let file = match resolve(root, path).await {
		Ok(file) => file,
		Err(refusal) => return status(refusal),
	};
	match tokio::fs::read(&file).await {
		Ok(content) => Response::builder()
			.header(CONTENT_TYPE, content_type(&file))
			.header(CACHE_CONTROL, "no-cache")
			.body(Full::new(Bytes::from(content)))
			.expect("the headers are valid"),
		Err(_) => status(StatusCode::NOT_FOUND),
	}
}

fn status(code: StatusCode) -> Response<Full<Bytes>> {
	let text = code.canonical_reason().unwrap_or_default();
	Response::builder()
		.status(code)
		.header(CONTENT_TYPE, "text/plain; charset=utf-8")
		.body(Full::new(Bytes::from(text)))
		.expect("the headers are valid")
}

/// Find the file of a request path, or the status that refuses it: 404 for a file that does not
/// exist, and 403 for a path that leaves `root`.
async fn resolve(root: &Path, path: &str) -> Result<PathBuf, StatusCode> {
	let decoded = percent_decode_str(path)
		.decode_utf8()
		.map_err(|_| StatusCode::NOT_FOUND)?;
	let mut file = root.to_path_buf();
	for segment in decoded
		.split('/')
		.filter(|segment| !segment.is_empty() && *segment != ".")
	{
		if segment == ".." || segment.contains(['\\', '\0']) {
			return Err(StatusCode::FORBIDDEN);
		}
		file.push(segment);
	}
	let not_found = |_| StatusCode::NOT_FOUND;
	if tokio::fs::metadata(&file).await.map_err(not_found)?.is_dir() {
		file.push("index.html");
	}
	let file = tokio::fs::canonicalize(&file).await.map_err(not_found)?;
	let root = tokio::fs::canonicalize(root).await.map_err(not_found)?;
	if file.starts_with(root) {
		Ok(file)
	} else {
		Err(StatusCode::FORBIDDEN)
	}
}

fn content_type(file: &Path) -> &'static str {
	match file.extension().and_then(|extension| extension.to_str()) {
		Some("html") => "text/html; charset=utf-8",
		Some("js") => "text/javascript; charset=utf-8",
		Some("wasm") => "application/wasm",
		Some("css") => "text/css; charset=utf-8",
		Some("json") => "application/json",
		Some("png") => "image/png",
		Some("svg") => "image/svg+xml",
		_ => "application/octet-stream",
	}
}

#[cfg(test)]
mod tests {
	use std::fs;

	use http_body_util::BodyExt;
	use hyper::StatusCode;
	use hyper::header::{CACHE_CONTROL, CONTENT_TYPE};

	use super::serve;

	#[tokio::test]
	async fn a_file_has_its_content_type_and_asks_for_a_check_on_each_load() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let web = directory.path().join("web");
		fs::create_dir(&web).expect("the web directory");
		let cases = [
			("index.html", "text/html; charset=utf-8"),
			("app.js", "text/javascript; charset=utf-8"),
			("app_bg.wasm", "application/wasm"),
			("style.css", "text/css; charset=utf-8"),
			("data.json", "application/json"),
			("icon.png", "image/png"),
			("icon.svg", "image/svg+xml"),
			("notes.txt", "application/octet-stream"),
		];
		for (name, _) in cases {
			fs::write(web.join(name), name).expect("a file");
		}
		for (name, expected) in cases {
			let response = serve(&web, &format!("/{name}")).await;
			assert_eq!(response.status(), StatusCode::OK, "{name} must be served");
			assert_eq!(response.headers()[CONTENT_TYPE], expected, "the content type of {name}");
			assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
			let body = response.into_body().collect().await.expect("a body").to_bytes();
			assert_eq!(body, name.as_bytes(), "the body of {name}");
		}
		let root = serve(&web, "/").await;
		assert_eq!(
			root.headers()[CONTENT_TYPE],
			"text/html; charset=utf-8",
			"the root path serves index.html"
		);
		assert_eq!(serve(&web, "/missing.js").await.status(), StatusCode::NOT_FOUND);
	}

	#[tokio::test]
	async fn a_path_that_leaves_the_web_directory_is_refused() {
		let directory = tempfile::tempdir().expect("a temporary directory");
		let web = directory.path().join("web");
		fs::create_dir(&web).expect("the web directory");
		fs::write(directory.path().join("secret.txt"), "secret").expect("a file outside");
		std::os::unix::fs::symlink(directory.path().join("secret.txt"), web.join("link.txt")).expect("a link");
		for path in [
			"/../secret.txt",
			"/%2e%2e/secret.txt",
			"/a/..%2fsecret.txt",
			"/..%5csecret.txt",
			"/link.txt",
		] {
			let response = serve(&web, path).await;
			assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path} must be refused");
			let body = response.into_body().collect().await.expect("a body").to_bytes();
			assert!(!body.starts_with(b"secret"), "{path} leaked the file");
		}
	}
}
