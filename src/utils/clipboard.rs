use anyhow::Result;
use wl_clipboard_rs::copy::{MimeType, Options as ClipboardOptions, Seat, Source};

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
