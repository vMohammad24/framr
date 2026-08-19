use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

pub(crate) fn render_node(requested: Option<&Path>) -> Result<PathBuf> {
	if let Some(path) = requested {
		return Ok(path.to_path_buf());
	}
	let mut nodes: Vec<PathBuf> = std::fs::read_dir("/dev/dri")
		.context("VAAPI and DMA-BUF require a DRM render node")?
		.filter_map(Result::ok)
		.map(|entry| entry.path())
		.filter(|path| {
			path.file_name()
				.is_some_and(|name| name.as_encoded_bytes().starts_with(b"renderD"))
		})
		.collect();
	nodes.sort();
	nodes
		.into_iter()
		.next()
		.ok_or_else(|| anyhow!("VAAPI and DMA-BUF require a DRM render node"))
}
