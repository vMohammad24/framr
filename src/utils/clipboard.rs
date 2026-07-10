use anyhow::Result;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use wl_clipboard_rs::copy::{MimeType, Options as ClipboardOptions, Seat, Source};

pub fn copy_file_uri(path: &Path) -> Result<()> {
	let abs = std::fs::canonicalize(path)?;
	let mut uri = String::from("file://");
	for &b in abs.as_os_str().as_bytes() {
		match b {
			b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
				uri.push(b as char)
			}
			_ => uri.push_str(&format!("%{:02X}", b)),
		}
	}
	uri.push_str("\r\n");
	copy_to_clipboard(uri.into_bytes(), "text/uri-list")
}

pub fn copy_to_clipboard(data: Vec<u8>, mime_type: &str) -> Result<()> {
	match unsafe { libc::fork() } {
		-1 => anyhow::bail!("fork failed"),
		0 => {
			let mut clipboard_opts = ClipboardOptions::new();
			clipboard_opts.foreground(true).seat(Seat::All);
			let _ = clipboard_opts.copy(
				Source::Bytes(data.into()),
				MimeType::Specific(mime_type.to_string()),
			);
			std::process::exit(0);
		}
		_ => {}
	}
	Ok(())
}
