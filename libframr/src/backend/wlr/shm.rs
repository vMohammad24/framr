use std::ops::Deref;
use std::os::fd::AsFd;
use anyhow::Result;
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_shm::{WlShm, Format as WlFormat};
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1;

use crate::buffer::create_shm_fd;
use crate::output::PixelFormat;

pub(crate) fn pixel_format_to_wl_shm(
fmt: PixelFormat) -> WlFormat {
	match fmt {
		PixelFormat::Argb8888 => WlFormat::Argb8888,
		PixelFormat::Xrgb8888 => WlFormat::Xrgb8888,
		PixelFormat::Abgr8888 => WlFormat::Abgr8888,
		PixelFormat::Xbgr8888 => WlFormat::Xbgr8888,
		PixelFormat::Abgr2101010 => WlFormat::Abgr2101010,
		PixelFormat::Xbgr2101010 => WlFormat::Xbgr2101010,
	}
}

pub(crate) fn allocate_shm_buffer<T>(
	shm: &WlShm,
	qh: &wayland_client::QueueHandle<T>,
	width: i32,
	height: i32,
	stride: i32,
	wl_fmt: WlFormat,
) -> Result<(WlBuffer, std::fs::File, memmap2::Mmap)>
where
	T: wayland_client::Dispatch<wayland_client::protocol::wl_shm_pool::WlShmPool, ()>
		+ wayland_client::Dispatch<WlBuffer, ()>
		+ 'static,
{
	let byte_size = stride * height;
	let fd = create_shm_fd()?;
	let file = std::fs::File::from(fd);
	file.set_len(byte_size as u64)?;

	let pool = shm.create_pool(file.as_fd(), byte_size, qh, ());
	let buffer = pool.create_buffer(0, width, height, stride, wl_fmt, qh, ());
	pool.destroy();

	let mmap = unsafe { memmap2::Mmap::map(&file)? };
	Ok((buffer, file, mmap))
}

pub(crate) struct WlBufferGuard(pub(crate) WlBuffer);
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

pub(crate) struct WlFrameGuard(pub(crate) ZwlrScreencopyFrameV1);
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

pub(crate) struct ShmPoolSlot {
	pub(crate) buffer: WlBuffer,
	pub(crate) _file: std::fs::File,
	pub(crate) mmap: std::sync::Arc<memmap2::Mmap>,
	pub(crate) in_use: bool,
}

impl Drop for ShmPoolSlot {
	fn drop(&mut self) {
		self.buffer.destroy();
	}
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ShmPoolError {
	#[error("No available slots in ShmPool")]
	PoolFull,
	#[error("Failed to allocate SHM buffer: {0}")]
	AllocationFailed(#[from] anyhow::Error),
}

pub(crate) struct ShmPool {
	pub(crate) slots: Vec<ShmPoolSlot>,
	pub(crate) max_slots: usize,
}

impl ShmPool {
	pub(crate) fn new(max_slots: usize) -> Self {
		Self {
			slots: Vec::with_capacity(max_slots),
			max_slots,
		}
	}

	pub(crate) fn get_slot<T>(
		&mut self,
		shm: &WlShm,
		qh: &wayland_client::QueueHandle<T>,
		width: i32,
		height: i32,
		stride: i32,
		wl_fmt: WlFormat,
	) -> Result<usize, ShmPoolError>
	where
		T: wayland_client::Dispatch<wayland_client::protocol::wl_shm_pool::WlShmPool, ()>
			+ wayland_client::Dispatch<WlBuffer, ()>
			+ 'static,
	{
		if let Some(idx) = self.slots.iter().position(|slot| !slot.in_use) {
			return Ok(idx);
		}

		if self.slots.len() < self.max_slots {
			let (buffer, file, mmap) = allocate_shm_buffer(shm, qh, width, height, stride, wl_fmt)?;
			self.slots.push(ShmPoolSlot {
				buffer,
				_file: file,
				mmap: std::sync::Arc::new(mmap),
				in_use: false,
			});
			return Ok(self.slots.len() - 1);
		}

		Err(ShmPoolError::PoolFull)
	}
}
