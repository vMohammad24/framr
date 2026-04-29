use crate::backend::CaptureBackend;
use crate::backend::wlr::WlrBackend;
use crate::output::{LogicalRegion, OutputInfo};
use anyhow::Result;
use image::RgbaImage;

pub struct FramrConnection {
	backend: Box<dyn CaptureBackend>,
}

impl FramrConnection {
	pub fn new() -> Result<Self> {
		Ok(Self {
			backend: Box::new(WlrBackend::new()?),
		})
	}

	pub fn get_all_outputs(&self) -> Result<Vec<OutputInfo>> {
		self.backend.get_outputs()
	}

	pub fn get_output(&self, index: usize) -> Result<OutputInfo> {
		let outputs = self.backend.get_outputs()?;
		outputs
			.get(index)
			.cloned()
			.ok_or_else(|| crate::error::FramrError::OutputNotFound(index).into())
	}

	pub fn screenshot_output(
		&self,
		output: &OutputInfo,
		include_cursor: bool,
	) -> Result<RgbaImage> {
		self.backend.capture_output(output, None, include_cursor)
	}

	pub fn screenshot_region(
		&self,
		region: &LogicalRegion,
		include_cursor: bool,
	) -> Result<RgbaImage> {
		self.backend.capture_region(region, include_cursor)
	}

	pub fn screenshot_all(&self, include_cursor: bool) -> Result<RgbaImage> {
		self.backend.capture_all_outputs(include_cursor)
	}
}
