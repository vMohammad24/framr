use anyhow::{Result, anyhow};
use ffmpeg::format::Pixel;
use ffmpeg::software::scaling::{context::Context, flag::Flags};
use ffmpeg::util::frame::video::Video;
use ffmpeg_next as ffmpeg;

use crate::{Frame, PixelFormat, Transform};

pub(crate) fn output_size(frame: &Frame) -> (u32, u32) {
	let width = frame.format.width.max(0) as u32;
	let height = frame.format.height.max(0) as u32;
	match frame.transform {
		Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270 => {
			(height, width)
		}
		_ => (width, height),
	}
}

pub(crate) fn is_ten_bit(format: PixelFormat) -> bool {
	matches!(format, PixelFormat::Abgr2101010 | PixelFormat::Xbgr2101010)
}

fn ffmpeg_pixel_format(format: PixelFormat) -> Pixel {
	match format {
		PixelFormat::Argb8888 => Pixel::BGRA,
		PixelFormat::Xrgb8888 => Pixel::BGRZ,
		PixelFormat::Abgr8888 => Pixel::RGBA,
		PixelFormat::Xbgr8888 => Pixel::RGBZ,
		PixelFormat::Abgr2101010 | PixelFormat::Xbgr2101010 => Pixel::X2BGR10LE,
	}
}

pub(crate) fn packed_format(frame: &Frame) -> Pixel {
	ffmpeg_pixel_format(frame.format.format)
}

fn copy_packed_frame(frame: &Frame, destination: &mut Video) -> Result<()> {
	let bytes = frame.bytes()?;
	let bytes = bytes.as_ref();
	let width = usize::try_from(frame.format.width)?;
	let height = usize::try_from(frame.format.height)?;
	let source_stride = usize::try_from(frame.format.stride)?;
	let row_size = width
		.checked_mul(frame.format.format.bytes_per_pixel())
		.ok_or_else(|| anyhow!("frame row is too wide"))?;
	let required = source_stride
		.checked_mul(height)
		.ok_or_else(|| anyhow!("frame is too large"))?;
	if source_stride < row_size || bytes.len() < required {
		return Err(anyhow!(
			"invalid frame layout: {} bytes, {} stride, {} byte rows",
			bytes.len(),
			source_stride,
			row_size
		));
	}
	let destination_stride = destination.stride(0);
	if destination_stride < row_size {
		return Err(anyhow!("FFmpeg allocated a frame with an invalid stride"));
	}
	let destination_data = destination.data_mut(0);
	for row in 0..height {
		let source_start = row * source_stride;
		let destination_start = row * destination_stride;
		destination_data[destination_start..destination_start + row_size]
			.copy_from_slice(&bytes[source_start..source_start + row_size]);
	}
	Ok(())
}

fn transformed_packed(frame: &Frame, width: u32, height: u32) -> Result<Video> {
	let bytes = frame.bytes()?;
	let bytes = bytes.as_ref();
	let source_width = usize::try_from(frame.format.width)?;
	let source_height = usize::try_from(frame.format.height)?;
	let source_stride = usize::try_from(frame.format.stride)?;
	let pixel_size = frame.format.format.bytes_per_pixel();
	let source_row_size = source_width
		.checked_mul(pixel_size)
		.ok_or_else(|| anyhow!("frame row is too wide"))?;
	let required = source_stride
		.checked_mul(source_height)
		.ok_or_else(|| anyhow!("frame is too large"))?;
	if source_stride < source_row_size || bytes.len() < required {
		return Err(anyhow!("invalid source frame layout"));
	}
	let mut video = Video::new(ffmpeg_pixel_format(frame.format.format), width, height);
	let destination_stride = video.stride(0);
	let destination_width = width as usize;
	let destination_height = height as usize;
	let destination = video.data_mut(0);
	for source_y in 0..source_height {
		for source_x in 0..source_width {
			let (destination_x, destination_y) = match frame.transform {
				Transform::_90 => (source_height - source_y - 1, source_x),
				Transform::_180 => (source_width - source_x - 1, source_height - source_y - 1),
				Transform::_270 => (source_y, source_width - source_x - 1),
				Transform::Flipped => (source_width - source_x - 1, source_y),
				Transform::Flipped90 => (source_height - source_y - 1, source_width - source_x - 1),
				Transform::Flipped180 => (source_x, source_height - source_y - 1),
				Transform::Flipped270 => (source_y, source_x),
				Transform::Normal => (source_x, source_y),
			};
			if destination_x >= destination_width || destination_y >= destination_height {
				return Err(anyhow!("invalid transformed frame dimensions"));
			}
			let source_start = source_y * source_stride + source_x * pixel_size;
			let destination_start = destination_y * destination_stride + destination_x * pixel_size;
			destination[destination_start..destination_start + pixel_size]
				.copy_from_slice(&bytes[source_start..source_start + pixel_size]);
		}
	}
	Ok(video)
}

pub(crate) fn packed_frame(frame: &Frame) -> Result<Video> {
	let (width, height) = output_size(frame);
	if frame.transform == Transform::Normal {
		let mut source = Video::new(
			ffmpeg_pixel_format(frame.format.format),
			frame.format.width as u32,
			frame.format.height as u32,
		);
		copy_packed_frame(frame, &mut source)?;
		Ok(source)
	} else {
		transformed_packed(frame, width, height)
	}
}

pub(crate) struct FrameConverter {
	width: u32,
	height: u32,
	frame_width: u32,
	frame_height: u32,
	target: Pixel,
	source: Option<(Pixel, u32, u32)>,
	scaler: Option<Context>,
}

impl FrameConverter {
	pub(crate) fn new(width: u32, height: u32, target: Pixel) -> Self {
		Self::new_padded(width, height, width, height, target)
	}

	pub(crate) fn new_padded(
		width: u32,
		height: u32,
		frame_width: u32,
		frame_height: u32,
		target: Pixel,
	) -> Self {
		Self {
			width,
			height,
			frame_width,
			frame_height,
			target,
			source: None,
			scaler: None,
		}
	}

	pub(crate) fn convert(&mut self, frame: &Frame) -> Result<Video> {
		let source = packed_frame(frame)?;
		let source_config = (source.format(), source.width(), source.height());
		if self.source != Some(source_config) {
			self.scaler = Some(Context::get(
				source.format(),
				source.width(),
				source.height(),
				self.target,
				self.width,
				self.height,
				Flags::BILINEAR,
			)?);
			self.source = Some(source_config);
		}
		let mut converted = Video::new(self.target, self.frame_width, self.frame_height);
		if self.width != self.frame_width || self.height != self.frame_height {
			let linesizes: [isize; 4] = std::array::from_fn(|index| unsafe {
				(*converted.as_ptr()).linesize[index] as isize
			});
			let result = unsafe {
				ffmpeg::ffi::av_image_fill_black(
					(*converted.as_mut_ptr()).data.as_ptr(),
					linesizes.as_ptr(),
					self.target.into(),
					ffmpeg::ffi::AVColorRange::AVCOL_RANGE_MPEG,
					self.frame_width as i32,
					self.frame_height as i32,
				)
			};
			if result < 0 {
				return Err(ffmpeg::Error::from(result).into());
			}
			unsafe {
				(*converted.as_mut_ptr()).width = self.width as i32;
				(*converted.as_mut_ptr()).height = self.height as i32;
			}
		}
		let result = self
			.scaler
			.as_mut()
			.ok_or_else(|| anyhow!("FFmpeg scaler is unavailable"))?
			.run(&source, &mut converted);
		unsafe {
			(*converted.as_mut_ptr()).width = self.frame_width as i32;
			(*converted.as_mut_ptr()).height = self.frame_height as i32;
		}
		result?;
		Ok(converted)
	}
}
