use crate::RecordingConfig;
use crate::output::{BgraImage, LogicalRegion, OutputInfo};
use anyhow::Result;
use image::RgbaImage;

pub struct RecordingHandle {
	pub stop_sender: crossbeam_channel::Sender<()>,
	pub pipeline_thread: std::thread::JoinHandle<Result<()>>,
}

pub trait CaptureBackend: Send + Sync {
	fn get_outputs(&self) -> Result<Vec<OutputInfo>>;
	fn capture_output(
		&self,
		output: &OutputInfo,
		region: Option<LogicalRegion>,
		include_cursor: bool,
	) -> Result<RgbaImage>;
	fn capture_all_outputs(&self, include_cursor: bool) -> Result<RgbaImage>;

	fn capture_each_output_bgra(
		&self,
		include_cursor: bool,
	) -> Result<Vec<(OutputInfo, BgraImage)>> {
		let outputs = self.get_outputs()?;
		let mut results = Vec::with_capacity(outputs.len());
		for output in outputs {
			let mut img = self.capture_output(&output, None, include_cursor)?;
			for pixel in img.as_mut().chunks_exact_mut(4) {
				pixel.swap(0, 2);
			}
			results.push((output, BgraImage(img)));
		}
		Ok(results)
	}

	fn start_recording_all(
		&self,
		include_cursor: bool,
		output_path: std::path::PathBuf,
		recording_config: RecordingConfig,
	) -> Result<RecordingHandle>;

	fn start_recording(
		&self,
		output: &OutputInfo,
		region: Option<LogicalRegion>,
		include_cursor: bool,
		output_path: std::path::PathBuf,
		recording_config: RecordingConfig,
	) -> Result<RecordingHandle>;

	fn start_recording_region(
		&self,
		region: &LogicalRegion,
		include_cursor: bool,
		output_path: std::path::PathBuf,
		recording_config: RecordingConfig,
	) -> Result<RecordingHandle> {
		let outputs = self.get_outputs()?;
		let containing = outputs.iter().find(|o| o.contains(region));

		if let Some(output) = containing {
			return self.start_recording(
				output,
				Some(*region),
				include_cursor,
				output_path,
				recording_config,
			);
		}

		self.start_recording_region_internal(region, include_cursor, output_path, recording_config)
	}

	fn start_recording_region_internal(
		&self,
		region: &LogicalRegion,
		include_cursor: bool,
		output_path: std::path::PathBuf,
		recording_config: RecordingConfig,
	) -> Result<RecordingHandle>;

	fn capture_region(&self, region: &LogicalRegion, include_cursor: bool) -> Result<RgbaImage> {
		let outputs = self.get_outputs()?;
		let containing = outputs.iter().find(|o| o.contains(region));

		if let Some(output) = containing {
			return self.capture_output(output, Some(*region), include_cursor);
		}

		let composite = self.capture_all_outputs(include_cursor)?;

		let bounds = crate::output::bounding_region(&outputs)
			.ok_or::<anyhow::Error>(crate::error::FramrError::NoOutputs.into())?;
		let min_x = bounds.position.x;
		let min_y = bounds.position.y;

		let composite_w = composite.width();
		let composite_h = composite.height();

		let scale_x = composite_w as f64 / bounds.size.width.max(1) as f64;
		let scale_y = composite_h as f64 / bounds.size.height.max(1) as f64;

		let to_px = |v: i64, origin: i32, scale: f64, limit: u32| {
			((v - origin as i64) as f64 * scale)
				.round()
				.clamp(0.0, limit as f64) as u32
		};
		let x0 = to_px(region.position.x as i64, min_x, scale_x, composite_w);
		let y0 = to_px(region.position.y as i64, min_y, scale_y, composite_h);
		let x1 = to_px(
			region.position.x as i64 + region.size.width as i64,
			min_x,
			scale_x,
			composite_w,
		);
		let y1 = to_px(
			region.position.y as i64 + region.size.height as i64,
			min_y,
			scale_y,
			composite_h,
		);

		if x0 >= x1 || y0 >= y1 {
			return Err(anyhow::anyhow!("selected region is outside all outputs"));
		}

		let mut composite = composite;
		Ok(image::imageops::crop(&mut composite, x0, y0, x1 - x0, y1 - y0).to_image())
	}
}

pub(crate) mod kde;
pub(crate) mod wlr;
