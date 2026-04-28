use std::os::fd::AsFd;

use anyhow::Result;
use image::{ImageBuffer, Rgba, RgbaImage};
use wayland_client::Connection;
use wayland_client::globals::GlobalList;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1;
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;

use crate::buffer::create_shm_fd;
use crate::convert::convert_to_rgba;
use crate::dispatch::{CaptureState, FrameState, MultiCaptureState};
use crate::error::FramrError;
use crate::output::{FrameFormat, LogicalRegion, OutputInfo};
use crate::transform::apply_transform;

fn do_capture(
	conn: &Connection,
	globals: &GlobalList,
	output: &OutputInfo,
	region: Option<&LogicalRegion>,
	include_cursor: bool,
) -> Result<RgbaImage> {
	let mut state = CaptureState::default();
	let mut event_queue = conn.new_event_queue::<CaptureState>();
	let qh = event_queue.handle();

	let screencopy_mgr: ZwlrScreencopyManagerV1 = globals
		.bind(&qh, 3..=3, ())
		.map_err(|_| FramrError::ProtocolNotSupported("wlr-screencopy".into()))?;

	let cursor_val = if include_cursor { 1 } else { 0 };

	let frame = if let Some(region) = region {
		let local_x = region.position.x - output.logical_position.x;
		let local_y = region.position.y - output.logical_position.y;
		screencopy_mgr.capture_output_region(
			cursor_val,
			&output.wl_output,
			local_x,
			local_y,
			region.size.width as i32,
			region.size.height as i32,
			&qh,
			(),
		)
	} else {
		screencopy_mgr.capture_output_region(
			cursor_val,
			&output.wl_output,
			0,
			0,
			output.logical_size.width as i32,
			output.logical_size.height as i32,
			&qh,
			(),
		)
	};

	while !state.buffer_done {
		event_queue.blocking_dispatch(&mut state)?;
	}

	let frame_format = state
		.formats
		.first()
		.ok_or(FramrError::NoSupportedBufferFormat)?
		.clone();

	let byte_size = frame_format.byte_size();
	let fd = create_shm_fd()?;

	let file = std::fs::File::from(fd);
	file.set_len(byte_size as u64)?;

	let shm: WlShm = globals.bind(&qh, 1..=1, ())?;
	let pool = shm.create_pool(file.as_fd(), byte_size as i32, &qh, ());
	let buffer = pool.create_buffer(
		0,
		frame_format.width,
		frame_format.height,
		frame_format.stride,
		frame_format.format,
		&qh,
		(),
	);

	frame.copy(&buffer);

	while state.frame_state == FrameState::Pending {
		event_queue.blocking_dispatch(&mut state)?;
	}

	if state.frame_state == FrameState::Failed {
		return Err(FramrError::FrameCaptureFailed.into());
	}

	let mmap = unsafe { memmap2::Mmap::map(&file)? };
	let mut raw = mmap.to_vec();

	convert_to_rgba(&mut raw, frame_format.format)
		.ok_or_else(|| anyhow::anyhow!("unsupported pixel format"))?;

	let width = frame_format.width as u32;
	let height = frame_format.height as u32;

	buffer.destroy();
	pool.destroy();

	let image = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, raw)
		.ok_or_else(|| anyhow::anyhow!("failed to create image buffer"))?;

	Ok(apply_transform(image, output.transform))
}

pub fn capture_output(
	conn: &Connection,
	globals: &GlobalList,
	output: &OutputInfo,
	include_cursor: bool,
) -> Result<RgbaImage> {
	do_capture(conn, globals, output, None, include_cursor)
}

pub fn capture_region(
	conn: &Connection,
	globals: &GlobalList,
	outputs: &[OutputInfo],
	region: &LogicalRegion,
	include_cursor: bool,
) -> Result<RgbaImage> {
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
		return do_capture(conn, globals, output, Some(region), include_cursor);
	}

	let composite = capture_all_outputs(conn, globals, outputs, include_cursor)?;

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

pub fn capture_all_outputs(
	conn: &Connection,
	globals: &GlobalList,
	outputs: &[OutputInfo],
	include_cursor: bool,
) -> Result<RgbaImage> {
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
	let max_x = outputs
		.iter()
		.map(|o| o.logical_position.x + o.logical_size.width as i32)
		.max()
		.unwrap_or(0);
	let max_y = outputs
		.iter()
		.map(|o| o.logical_position.y + o.logical_size.height as i32)
		.max()
		.unwrap_or(0);

	let total_width = (max_x - min_x) as u32;
	let total_height = (max_y - min_y) as u32;

	let mut state = MultiCaptureState::new(outputs.len());
	let mut event_queue = conn.new_event_queue::<MultiCaptureState>();
	let qh = event_queue.handle();

	let screencopy_mgr: ZwlrScreencopyManagerV1 = globals
		.bind(&qh, 3..=3, ())
		.map_err(|_| FramrError::ProtocolNotSupported("wlr-screencopy".into()))?;

	let cursor_val = if include_cursor { 1 } else { 0 };

	let frames: Vec<ZwlrScreencopyFrameV1> = outputs
		.iter()
		.enumerate()
		.map(|(i, output)| screencopy_mgr.capture_output(cursor_val, &output.wl_output, &qh, i))
		.collect();

	while !state.all_buffer_done() {
		event_queue.blocking_dispatch(&mut state)?;
	}

	let mut buffers_files: Vec<(
		wayland_client::protocol::wl_buffer::WlBuffer,
		std::fs::File,
		FrameFormat,
	)> = Vec::with_capacity(outputs.len());

	for (i, slot) in state.slots.iter().enumerate() {
		let frame_format = slot
			.formats
			.first()
			.ok_or(FramrError::NoSupportedBufferFormat)?
			.clone();

		let byte_size = frame_format.byte_size();
		let fd = create_shm_fd()?;
		let file = std::fs::File::from(fd);
		file.set_len(byte_size as u64)?;

		let shm: WlShm = globals.bind(&qh, 1..=1, ())?;
		let pool = shm.create_pool(file.as_fd(), byte_size as i32, &qh, ());
		let buffer = pool.create_buffer(
			0,
			frame_format.width,
			frame_format.height,
			frame_format.stride,
			frame_format.format,
			&qh,
			(),
		);

		frames[i].copy(&buffer);
		buffers_files.push((buffer, file, frame_format));
	}

	while !state.all_finished() {
		event_queue.blocking_dispatch(&mut state)?;
	}

	let mut composite = RgbaImage::new(total_width, total_height);

	for (i, output) in outputs.iter().enumerate() {
		if state.slots[i].frame_state == FrameState::Failed {
			return Err(FramrError::FrameCaptureFailed.into());
		}

		let (_buffer, file, frame_format) = &buffers_files[i];
		let mmap = unsafe { memmap2::Mmap::map(file)? };

		let mut raw = mmap.to_vec();

		convert_to_rgba(&mut raw, frame_format.format)
			.ok_or_else(|| anyhow::anyhow!("unsupported pixel format"))?;

		let width = frame_format.width as u32;
		let height = frame_format.height as u32;

		let image = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, raw)
			.ok_or_else(|| anyhow::anyhow!("failed to create image buffer"))?;

		let image = apply_transform(image, output.transform);

		let x = (output.logical_position.x - min_x) as u64;
		let y = (output.logical_position.y - min_y) as u64;

		for (px, py, pixel) in image.enumerate_pixels() {
			let dx = px as u64 + x;
			let dy = py as u64 + y;
			if dx < total_width as u64 && dy < total_height as u64 {
				composite.put_pixel(dx as u32, dy as u32, *pixel);
			}
		}
	}

	Ok(composite)
}
