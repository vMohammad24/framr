use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use memmap2::{Mmap, MmapOptions};

use crate::{FrameFormat, Transform};

#[derive(Debug)]
pub struct DmaBufPlane {
	pub fd: Arc<OwnedFd>,
	pub offset: u32,
	pub stride: u32,
}

impl DmaBufPlane {
	pub fn try_clone(&self) -> Result<Self> {
		Ok(Self {
			fd: Arc::new(self.fd.as_fd().try_clone_to_owned()?),
			offset: self.offset,
			stride: self.stride,
		})
	}
}

#[derive(Debug)]
pub struct DmaBuf {
	pub planes: Vec<DmaBufPlane>,
	pub modifier: u64,
}

impl DmaBuf {
	pub fn try_clone(&self) -> Result<Self> {
		Ok(Self {
			planes: self
				.planes
				.iter()
				.map(DmaBufPlane::try_clone)
				.collect::<Result<_>>()?,
			modifier: self.modifier,
		})
	}
}

#[derive(Debug, Clone)]
pub enum FrameData {
	SharedMemory(Arc<Mmap>),
	DmaBuf(Arc<DmaBuf>),
	Owned(Arc<[u8]>),
}

#[derive(Debug, Clone)]
pub struct Frame {
	pub format: FrameFormat,
	pub transform: Transform,
	pub timestamp: Duration,
	pub data: FrameData,
}

impl Frame {
	pub fn from_shm(
		data: Arc<Mmap>,
		format: FrameFormat,
		transform: Transform,
		timestamp: Duration,
	) -> Self {
		Self {
			format,
			transform,
			timestamp,
			data: FrameData::SharedMemory(data),
		}
	}

	pub fn from_dmabuf(
		data: DmaBuf,
		format: FrameFormat,
		transform: Transform,
		timestamp: Duration,
	) -> Self {
		Self {
			format,
			transform,
			timestamp,
			data: FrameData::DmaBuf(Arc::new(data)),
		}
	}

	pub fn from_owned(
		data: Vec<u8>,
		format: FrameFormat,
		transform: Transform,
		timestamp: Duration,
	) -> Self {
		Self {
			format,
			transform,
			timestamp,
			data: FrameData::Owned(data.into()),
		}
	}

	pub fn bytes(&self) -> Result<FrameBytes<'_>> {
		match &self.data {
			FrameData::SharedMemory(data) => Ok(FrameBytes::Borrowed(data)),
			FrameData::Owned(data) => Ok(FrameBytes::Borrowed(data)),
			FrameData::DmaBuf(data) => {
				if data.planes.len() != 1 {
					return Err(anyhow!(
						"CPU byte access requires a single-plane packed DMA-BUF frame"
					));
				}
				let plane = data
					.planes
					.first()
					.ok_or_else(|| anyhow!("DMA-BUF frame has no planes"))?;
				let length = usize::try_from(self.format.stride)?
					.checked_mul(usize::try_from(self.format.height)?)
					.ok_or_else(|| anyhow!("DMA-BUF frame is too large"))?;
				dmabuf_sync(plane.fd.as_raw_fd(), false)?;
				let mmap = unsafe {
					MmapOptions::new()
						.offset(u64::from(plane.offset))
						.len(length)
						.map(plane.fd.as_ref())
				};
				match mmap {
					Ok(mmap) => Ok(FrameBytes::Mapped {
						mmap,
						fd: plane.fd.clone(),
					}),
					Err(error) => {
						let _ = dmabuf_sync(plane.fd.as_raw_fd(), true);
						Err(error.into())
					}
				}
			}
		}
	}
}

#[repr(C)]
struct DmaBufSync {
	flags: u64,
}

fn dmabuf_sync(fd: std::os::fd::RawFd, end: bool) -> std::io::Result<()> {
	let sync = DmaBufSync {
		flags: 1 | if end { 4 } else { 0 },
	};
	let result = unsafe { libc::ioctl(fd, 0x4008_6200, &sync) };
	if result == -1 {
		Err(std::io::Error::last_os_error())
	} else {
		Ok(())
	}
}

pub enum FrameBytes<'a> {
	Borrowed(&'a [u8]),
	Mapped { mmap: Mmap, fd: Arc<OwnedFd> },
}

impl AsRef<[u8]> for FrameBytes<'_> {
	fn as_ref(&self) -> &[u8] {
		match self {
			Self::Borrowed(data) => data,
			Self::Mapped { mmap, .. } => mmap,
		}
	}
}

impl Drop for FrameBytes<'_> {
	fn drop(&mut self) {
		if let Self::Mapped { fd, .. } = self {
			let _ = dmabuf_sync(fd.as_raw_fd(), true);
		}
	}
}
