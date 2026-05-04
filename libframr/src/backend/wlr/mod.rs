use std::ops::Deref;
use std::os::fd::AsFd;

use anyhow::Result;
use image::{ImageBuffer, Rgba, RgbaImage};
use wayland_client::Connection;
use wayland_client::globals::{GlobalList, registry_queue_init};
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1;
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;

use crate::backend::CaptureBackend;
use crate::buffer::create_shm_fd;
use crate::convert::convert_to_rgba;
use crate::error::FramrError;
use crate::output::{FrameFormat, LogicalRegion, OutputInfo, PixelFormat};
use crate::transform::apply_transform;

mod dispatch;
use dispatch::*;

pub struct WlrBackend {
	pub(crate) conn: Connection,
	pub(crate) globals: GlobalList,
	outputs: Vec<OutputInfo>,
	wl_outputs: Vec<WlOutput>,
}

struct WlBufferGuard(WlBuffer);
impl Drop for WlBufferGuard {
	fn drop(&mut self) {
		self.0.destroy();
	}
}
impl Deref for WlBufferGuard {
	type Target = WlBuffer;
	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

struct WlFrameGuard(ZwlrScreencopyFrameV1);
impl Drop for WlFrameGuard {
	fn drop(&mut self) {
		self.0.destroy();
	}
}
impl Deref for WlFrameGuard {
	type Target = ZwlrScreencopyFrameV1;
	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl WlrBackend {
	pub fn new() -> Result<Self> {
		let conn = Connection::connect_to_env()
			.map_err(|e| FramrError::ConnectionFailed(format!("{e}")))?;
		let (globals, _) = registry_queue_init::<RegistryState>(&conn)
			.map_err(|e| FramrError::ConnectionFailed(format!("{e}")))?;

		if !globals
			.contents()
			.clone_list()
			.iter()
			.any(|g| g.interface == "zwlr_screencopy_manager_v1")
		{
			return Err(FramrError::ProtocolNotSupported("wlr-screencopy".into()).into());
		}

		let mut this = Self {
			conn,
			globals,
			outputs: Vec::new(),
			wl_outputs: Vec::new(),
		};
		this.refresh_outputs()?;
		Ok(this)
	}

	pub(crate) fn new_without_screencopy() -> Result<Self> {
		let conn = Connection::connect_to_env()
			.map_err(|e| FramrError::ConnectionFailed(format!("{e}")))?;
		let (globals, _) = registry_queue_init::<RegistryState>(&conn)
			.map_err(|e| FramrError::ConnectionFailed(format!("{e}")))?;

		let mut this = Self {
			conn,
			globals,
			outputs: Vec::new(),
			wl_outputs: Vec::new(),
		};
		this.refresh_outputs()?;
		Ok(this)
	}

	fn refresh_outputs(&mut self) -> Result<()> {
		let mut state = OutputEnumState::default();
		let mut event_queue = self.conn.new_event_queue::<OutputEnumState>();
		let qh = event_queue.handle();

		let _ = self.conn.display().get_registry(&qh, ());
		event_queue.roundtrip(&mut state)?;

		let Ok(xdg_mgr): Result<ZxdgOutputManagerV1, _> = self.globals.bind(&qh, 3..=3, ()) else {
			self.update_outputs(state.outputs);
			return Ok(());
		};

		let xdg_outputs: Vec<_> = state
			.outputs
			.iter()
			.enumerate()
			.map(|(i, output)| xdg_mgr.get_xdg_output(&output.wl_output, &qh, i))
			.collect();
		event_queue.roundtrip(&mut state)?;

		for xdg in &xdg_outputs {
			xdg.destroy();
		}

		self.update_outputs(state.outputs);

		if self.outputs.is_empty() {
			return Err(FramrError::NoOutputs.into());
		}

		Ok(())
	}

	fn update_outputs(&mut self, partials: Vec<PartialOutput>) {
		self.wl_outputs = partials.iter().map(|p| p.wl_output.clone()).collect();
		self.outputs = partials
			.into_iter()
			.enumerate()
			.map(|(id, p)| OutputInfo {
				id,
				name: p.name,
				description: p.description,
				logical_position: p.logical_position,
				logical_size: p.logical_size,
				physical_size: p.physical_size,
				transform: convert_transform(p.transform),
				scale: p.scale,
			})
			.collect();
	}
}

impl CaptureBackend for WlrBackend {
	fn get_outputs(&self) -> Result<Vec<OutputInfo>> {
		Ok(self.outputs.clone())
	}

	fn capture_output(
		&self,
		output: &OutputInfo,
		region: Option<LogicalRegion>,
		include_cursor: bool,
	) -> Result<RgbaImage> {
		let wl_output = self
			.wl_outputs
			.get(output.id)
			.ok_or_else(|| anyhow::anyhow!("WlOutput not found for id {}", output.id))?;

		let mut state = CaptureState::default();
		let mut event_queue = self.conn.new_event_queue::<CaptureState>();
		let qh = event_queue.handle();

		let screencopy_mgr: ZwlrScreencopyManagerV1 = self
			.globals
			.bind(&qh, 3..=3, ())
			.map_err(|_| FramrError::ProtocolNotSupported("wlr-screencopy".into()))?;

		let cursor_val = if include_cursor { 1 } else { 0 };

		let frame = WlFrameGuard(if let Some(region) = region {
			let local_x = region.position.x - output.logical_position.x;
			let local_y = region.position.y - output.logical_position.y;
			screencopy_mgr.capture_output_region(
				cursor_val,
				wl_output,
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
				wl_output,
				0,
				0,
				output.logical_size.width as i32,
				output.logical_size.height as i32,
				&qh,
				(),
			)
		});

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

		let shm: WlShm = self.globals.bind(&qh, 1..=1, ())?;
		let pool = shm.create_pool(file.as_fd(), byte_size as i32, &qh, ());

		// Map our PixelFormat back to Wayland format for buffer creation
		let wl_fmt = match frame_format.format {
			PixelFormat::Argb8888 => wayland_client::protocol::wl_shm::Format::Argb8888,
			PixelFormat::Xrgb8888 => wayland_client::protocol::wl_shm::Format::Xrgb8888,
			PixelFormat::Abgr8888 => wayland_client::protocol::wl_shm::Format::Abgr8888,
			PixelFormat::Xbgr8888 => wayland_client::protocol::wl_shm::Format::Xbgr8888,
			PixelFormat::Abgr2101010 => wayland_client::protocol::wl_shm::Format::Abgr2101010,
			PixelFormat::Xbgr2101010 => wayland_client::protocol::wl_shm::Format::Xbgr2101010,
		};

		let buffer = WlBufferGuard(pool.create_buffer(
			0,
			frame_format.width,
			frame_format.height,
			frame_format.stride,
			wl_fmt,
			&qh,
			(),
		));
		pool.destroy();

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

		let image = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, raw)
			.ok_or_else(|| anyhow::anyhow!("failed to create image buffer"))?;

		Ok(apply_transform(image, output.transform))
	}

	fn capture_all_outputs(&self, include_cursor: bool) -> Result<RgbaImage> {
		let outputs = &self.outputs;
		if outputs.is_empty() {
			return Err(FramrError::NoOutputs.into());
		}

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
		let mut event_queue = self.conn.new_event_queue::<MultiCaptureState>();
		let qh = event_queue.handle();

		let screencopy_mgr: ZwlrScreencopyManagerV1 = self
			.globals
			.bind(&qh, 3..=3, ())
			.map_err(|_| FramrError::ProtocolNotSupported("wlr-screencopy".into()))?;

		let cursor_val = if include_cursor { 1 } else { 0 };

		let frames: Vec<WlFrameGuard> = outputs
			.iter()
			.map(|output| {
				let wl_output = &self.wl_outputs[output.id];
				WlFrameGuard(screencopy_mgr.capture_output(cursor_val, wl_output, &qh, output.id))
			})
			.collect();

		while !state.all_buffer_done() {
			event_queue.blocking_dispatch(&mut state)?;
		}

		let mut buffers_files: Vec<(WlBufferGuard, std::fs::File, FrameFormat)> =
			Vec::with_capacity(outputs.len());

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

			let shm: WlShm = self.globals.bind(&qh, 1..=1, ())?;
			let pool = shm.create_pool(file.as_fd(), byte_size as i32, &qh, ());

			let wl_fmt = match frame_format.format {
				PixelFormat::Argb8888 => wayland_client::protocol::wl_shm::Format::Argb8888,
				PixelFormat::Xrgb8888 => wayland_client::protocol::wl_shm::Format::Xrgb8888,
				PixelFormat::Abgr8888 => wayland_client::protocol::wl_shm::Format::Abgr8888,
				PixelFormat::Xbgr8888 => wayland_client::protocol::wl_shm::Format::Xbgr8888,
				PixelFormat::Abgr2101010 => wayland_client::protocol::wl_shm::Format::Abgr2101010,
				PixelFormat::Xbgr2101010 => wayland_client::protocol::wl_shm::Format::Xbgr2101010,
			};

			let buffer = WlBufferGuard(pool.create_buffer(
				0,
				frame_format.width,
				frame_format.height,
				frame_format.stride,
				wl_fmt,
				&qh,
				(),
			));
			pool.destroy();

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
}
