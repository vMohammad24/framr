use crate::output::{LogicalRegion, OutputInfo};
use anyhow::Result;
use image::RgbaImage;

pub trait CaptureBackend: Send + Sync {
	fn get_outputs(&self) -> Result<Vec<OutputInfo>>;
	fn capture_output(
		&self,
		output: &OutputInfo,
		region: Option<LogicalRegion>,
		include_cursor: bool,
	) -> Result<RgbaImage>;
	fn capture_all_outputs(&self, include_cursor: bool) -> Result<RgbaImage>;

	fn capture_region(&self, region: &LogicalRegion, include_cursor: bool) -> Result<RgbaImage> {
		let outputs = self.get_outputs()?;
		let containing = outputs.iter().find(|o| {
			let ox = o.logical_position.x;
			let oy = o.logical_position.y;
			let ow = o.logical_size.width as i32;
			let oh = o.logical_size.height as i32;
			region.position.x >= ox
				&& region.position.y >= oy
				&& region.position.x + region.size.width as i32 <= ox + ow
				&& region.position.y + region.size.height as i32 <= oy + oh
		});

		if let Some(output) = containing {
			return self.capture_output(output, Some(*region), include_cursor);
		}

		let composite = self.capture_all_outputs(include_cursor)?;

		let min_x = outputs
			.iter()
			.map(|o| o.logical_position.x)
			.min()
			.unwrap_or(0);
		let min_y = outputs
			.iter()
			.map(|o| o.logical_position.y)
			.min()
			.unwrap_or(0);

		let composite_w = composite.width();
		let composite_h = composite.height();

		let crop_x = (region.position.x - min_x).max(0) as u32;
		let crop_y = (region.position.y - min_y).max(0) as u32;

		if crop_x >= composite_w || crop_y >= composite_h {
			return Err(anyhow::anyhow!("selected region is outside all outputs"));
		}

		let crop_w = region.size.width.min(composite_w.saturating_sub(crop_x));
		let crop_h = region.size.height.min(composite_h.saturating_sub(crop_y));

		let mut composite = composite;
		Ok(image::imageops::crop(&mut composite, crop_x, crop_y, crop_w, crop_h).to_image())
	}
}

pub mod kde;
pub mod wlr;
