use std::fs::{File, OpenOptions};
use std::os::fd::AsFd;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use drm_fourcc::DrmFourcc;
use gbm::{BufferObjectFlags, Device};
use wayland_client::QueueHandle;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1;

use crate::backend::wlr::dispatch::CaptureState;
use crate::backend::wlr::shm::WlBufferGuard;
use crate::{DmaBuf, DmaBufPlane, FrameFormat, PixelFormat};

pub(crate) fn convert_format(format: DrmFourcc) -> Option<PixelFormat> {
	match format {
		DrmFourcc::Argb8888 => Some(PixelFormat::Argb8888),
		DrmFourcc::Xrgb8888 => Some(PixelFormat::Xrgb8888),
		DrmFourcc::Abgr8888 => Some(PixelFormat::Abgr8888),
		DrmFourcc::Xbgr8888 => Some(PixelFormat::Xbgr8888),
		DrmFourcc::Abgr2101010 => Some(PixelFormat::Abgr2101010),
		DrmFourcc::Xbgr2101010 => Some(PixelFormat::Xbgr2101010),
		_ => None,
	}
}

fn drm_format(format: PixelFormat) -> DrmFourcc {
	match format {
		PixelFormat::Argb8888 => DrmFourcc::Argb8888,
		PixelFormat::Xrgb8888 => DrmFourcc::Xrgb8888,
		PixelFormat::Abgr8888 => DrmFourcc::Abgr8888,
		PixelFormat::Xbgr8888 => DrmFourcc::Xbgr8888,
		PixelFormat::Abgr2101010 => DrmFourcc::Abgr2101010,
		PixelFormat::Xbgr2101010 => DrmFourcc::Xbgr2101010,
	}
}

fn open_device(requested: Option<&Path>) -> Result<File> {
	let path = crate::gpu::render_node(requested)?;
	OpenOptions::new()
		.read(true)
		.write(true)
		.open(&path)
		.with_context(|| format!("failed to open DRM render node {}", path.display()))
}

pub(crate) struct DmaBufSlot {
	pub(crate) buffer: WlBufferGuard,
	pub(crate) data: Arc<DmaBuf>,
	pub(crate) format: FrameFormat,
	pub(crate) in_use: bool,
}

pub(crate) struct DmaBufPool {
	device: Device<File>,
	pub(crate) slots: Vec<DmaBufSlot>,
	capacity: usize,
}

impl DmaBufPool {
	pub(crate) fn new(device: Option<&Path>, capacity: usize) -> Result<Self> {
		Ok(Self {
			device: Device::new(open_device(device)?)?,
			slots: Vec::with_capacity(capacity),
			capacity,
		})
	}

	pub(crate) fn get_slot(
		&mut self,
		dmabuf: &ZwpLinuxDmabufV1,
		qh: &QueueHandle<CaptureState>,
		format: FrameFormat,
	) -> Result<usize> {
		if let Some((index, slot)) = self
			.slots
			.iter_mut()
			.enumerate()
			.find(|(_, slot)| !slot.in_use)
		{
			slot.in_use = true;
			return Ok(index);
		}
		if self.slots.len() == self.capacity {
			return Err(anyhow!("DMA-BUF pool is full"));
		}
		let bo = self
			.device
			.create_buffer_object::<()>(
				format.width as u32,
				format.height as u32,
				drm_format(format.format),
				BufferObjectFlags::LINEAR | BufferObjectFlags::RENDERING,
			)
			.context("failed to allocate a linear GBM capture buffer")?;
		let modifier: u64 = bo.modifier().into();
		let params = dmabuf.create_params(qh, ());
		let mut planes = Vec::with_capacity(bo.plane_count() as usize);
		for plane_index in 0..bo.plane_count() {
			let fd = bo.fd_for_plane(plane_index as i32)?;
			let offset = bo.offset(plane_index as i32);
			let stride = bo.stride_for_plane(plane_index as i32);
			params.add(
				fd.as_fd(),
				plane_index,
				offset,
				stride,
				(modifier >> 32) as u32,
				modifier as u32,
			);
			planes.push(DmaBufPlane {
				fd: Arc::new(fd),
				offset,
				stride,
			});
		}
		let buffer = params.create_immed(
			format.width,
			format.height,
			drm_format(format.format) as u32,
			wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1::Flags::empty(),
			qh,
			(),
		);
		params.destroy();
		let format = FrameFormat {
			stride: bo.stride() as i32,
			..format
		};
		let index = self.slots.len();
		self.slots.push(DmaBufSlot {
			buffer: WlBufferGuard(buffer),
			data: Arc::new(DmaBuf { planes, modifier }),
			format,
			in_use: true,
		});
		Ok(index)
	}
}
