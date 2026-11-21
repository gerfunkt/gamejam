//! Publish the built game to the branch that the workflow "Publish the game" deploys to GitHub Pages.
//!
//! The game builds on this machine. The branch holds only the files of `web/`, in one commit
//! without history, and each publication replaces it whole. The workflow copies the branch to Pages.

use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use anyhow::{Context, Result, bail, ensure};

/// The branch that holds the built game. It is a throwaway: each publication replaces it.
pub const BRANCH: &str = "pages-build";
/// The game server of the local network, which the published page offers for the multiplayer game.
pub const DEFAULT_SERVER: &str = "https://192.168.0.87:5201";
/// The empty setting of `web/index.html` that takes the address of the game server.
const EMPTY_SETTING: &str = r#"<meta name="game-server" content="">"#;

/// Copy the built game into `target/pages`, name `server` in its page, and push it to [`BRANCH`].
///
/// # Errors
///
/// Returns an error if `server` is not a plain HTTPS address, if the page has no setting for it,
/// or if a git command fails.
pub fn run(root: &Path, server: &str) -> Result<()> {
	check_address(server)?;
	let stage = root.join("target/pages");
	if stage.exists() {
		fs::remove_dir_all(&stage).with_context(|| format!("cannot empty {}", stage.display()))?;
	}
	copy(&root.join("web"), &stage)?;
	let index = stage.join("index.html");
	let page = fs::read_to_string(&index).context("cannot read the page of the game")?;
	ensure!(
		page.contains(EMPTY_SETTING),
		"web/index.html has no empty setting {EMPTY_SETTING} for the game server"
	);
	let named = format!(r#"<meta name="game-server" content="{server}">"#);
	fs::write(&index, page.replace(EMPTY_SETTING, &named))?;

	// A separate index leaves the index and the files of the checkout as they are.
	let git_dir = PathBuf::from(git(root, None, &["rev-parse", "--absolute-git-dir"])?);
	let index_file = root.join("target/pages.index");
	let _ = fs::remove_file(&index_file);
	let at_stage = |args: &[&str]| {
		let mut all = vec![
			format!("--git-dir={}", git_dir.display()),
			format!("--work-tree={}", stage.display()),
		];
		all.extend(args.iter().map(|arg| (*arg).to_owned()));
		let all: Vec<&str> = all.iter().map(String::as_str).collect();
		git(&stage, Some(&index_file), &all)
	};
	at_stage(&["add", "--all", "--force", "."])?;
	let tree = at_stage(&["write-tree"])?;
	let source = git(root, None, &["rev-parse", "--short", "HEAD"])?;
	let commit = at_stage(&[
		"commit-tree",
		&tree,
		"-m",
		&format!("build: the game of commit {source}"),
	])?;
	println!("pushing the built game to the branch {BRANCH}");
	git(
		root,
		None,
		&["push", "--force", "origin", &format!("{commit}:refs/heads/{BRANCH}")],
	)?;
	let _ = fs::remove_file(&index_file);
	println!(
		"published commit {source} to the branch {BRANCH}, with the game server {server}.\n\
		 Deploy it on GitHub: Actions, \"Publish the game\", \"Run workflow\"."
	);
	Ok(())
}

/// Check that `server` is a plain HTTPS address, because it goes into the page as it is.
fn check_address(server: &str) -> Result<()> {
	let Some(rest) = server.strip_prefix("https://") else {
		bail!("the game server must be an address such as {DEFAULT_SERVER}");
	};
	let rest = rest.strip_suffix('/').unwrap_or(rest);
	ensure!(
		!rest.is_empty()
			&& rest
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':')),
		"the game server must be an address such as {DEFAULT_SERVER}"
	);
	Ok(())
}

/// Copy the directory `from` and everything in it to `to`.
fn copy(from: &Path, to: &Path) -> Result<()> {
	fs::create_dir_all(to).with_context(|| format!("cannot create {}", to.display()))?;
	for entry in fs::read_dir(from).with_context(|| format!("cannot read {}", from.display()))? {
		let entry = entry?;
		let target = to.join(entry.file_name());
		if entry.file_type()?.is_dir() {
			copy(&entry.path(), &target)?;
		} else {
			fs::copy(entry.path(), &target).with_context(|| format!("cannot copy {}", entry.path().display()))?;
		}
	}
	Ok(())
}

/// Run git in `directory`, with `index` as its index file if given, and return what it printed.
fn git(directory: &Path, index: Option<&Path>, args: &[&str]) -> Result<String> {
	let mut command = Command::new("git");
	command.current_dir(directory).args(args);
	if let Some(index) = index {
		command.env("GIT_INDEX_FILE", index);
	}
	let output = command.output().context("cannot run git")?;
	ensure!(
		output.status.success(),
		"git {} failed: {}",
		args.join(" "),
		String::from_utf8_lossy(&output.stderr).trim()
	);
	Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn only_a_plain_https_address_may_name_the_game_server() {
		assert!(check_address(DEFAULT_SERVER).is_ok(), "the default address");
		assert!(
			check_address("https://game.local/").is_ok(),
			"a name with a slash at the end"
		);
		assert!(check_address("http://192.168.0.87:5201").is_err(), "plain HTTP");
		assert!(
			check_address(r#"https://x" onload="alert(1)"#).is_err(),
			"an address that would break out of the page"
		);
		assert!(check_address("https://").is_err(), "no host");
	}

	#[test]
	fn a_copy_holds_every_file_of_every_directory() {
		let scratch = std::env::temp_dir().join(format!("xtask-copy-{}", std::process::id()));
		let (from, to) = (scratch.join("from"), scratch.join("to"));
		fs::create_dir_all(from.join("deep")).unwrap();
		fs::write(from.join("top.txt"), "a").unwrap();
		fs::write(from.join("deep/inner.txt"), "b").unwrap();
		copy(&from, &to).unwrap();
		assert_eq!(
			fs::read_to_string(to.join("top.txt")).unwrap(),
			"a",
			"a file at the top"
		);
		assert_eq!(
			fs::read_to_string(to.join("deep/inner.txt")).unwrap(),
			"b",
			"a file in a directory"
		);
		fs::remove_dir_all(&scratch).unwrap();
	}
}
