use crate::backend::CaptureBackend;
use crate::convert::convert_to_rgba;
use crate::output::{LogicalRegion, OutputInfo, PixelFormat};
use anyhow::{Result, anyhow};
use dbus::arg::{self, RefArg};
use dbus::blocking::SyncConnection;
use drm_fourcc::DrmFourcc;
use image::{ImageBuffer, Rgba, RgbaImage};
use std::collections::HashMap;
use std::io::Read;
use std::os::unix::io::IntoRawFd;
use std::time::Duration;

pub struct KdeBackend {
	wayland_state: crate::backend::wlr::WlrBackend,
	dbus_conn: SyncConnection,
}

impl KdeBackend {
	pub fn new() -> Result<Self> {
		let dbus_conn = SyncConnection::new_session()
			.map_err(|e| anyhow!("Failed to connect to session D-Bus: {}", e))?;

		let wayland_state = crate::backend::wlr::WlrBackend::new_without_screencopy()?;

		Ok(Self {
			wayland_state,
			dbus_conn,
		})
	}

	fn perform_kwin_capture<F>(&self, capture_call: F) -> Result<RgbaImage>
	where
		F: FnOnce(
			&dbus::blocking::Proxy<&SyncConnection>,
			arg::OwnedFd,
		) -> Result<arg::PropMap, dbus::Error>,
	{
		let (read_pipe, write_pipe) =
			rustix::pipe::pipe().map_err(|e| anyhow!("Failed to create pipe: {}", e))?;

		let mut read_file = std::fs::File::from(read_pipe);

		let pipe_handle = std::thread::spawn(move || {
			let mut buf = Vec::new();
			read_file.read_to_end(&mut buf).map(|_| buf)
		});

		let proxy = self.dbus_conn.with_proxy(
			"org.kde.KWin",
			"/org/kde/KWin/ScreenShot2",
			Duration::from_secs(5),
		);

		let write_fd = unsafe { arg::OwnedFd::new(write_pipe.into_raw_fd()) };
		let dict =
			capture_call(&proxy, write_fd).map_err(|e| anyhow!("KWin D-Bus call failed: {}", e))?;

		let raw_data = pipe_handle
			.join()
			.map_err(|_| anyhow!("Pipe thread panicked"))?
			.map_err(|e| anyhow!("Failed to read from KWin pipe: {}", e))?;

		self.process_kwin_data(raw_data, dict)
	}

	fn process_kwin_data(&self, mut raw: Vec<u8>, dict: arg::PropMap) -> Result<RgbaImage> {
		let get_u32 = |key: &str| -> Result<u32> {
			dict.get(key)
				.ok_or_else(|| anyhow!("No {}", key))?
				.as_u64()
				.ok_or_else(|| anyhow!("Invalid {} type", key))
				.map(|v| v as u32)
		};

		let width = get_u32("width")?;
		let height = get_u32("height")?;
		let stride = get_u32("stride")?;
		let format_raw = get_u32("format")?;

		let pixel_format = if let Ok(fourcc) = DrmFourcc::try_from(format_raw) {
			match fourcc {
				DrmFourcc::Argb8888 => PixelFormat::Argb8888,
				DrmFourcc::Xrgb8888 => PixelFormat::Xrgb8888,
				DrmFourcc::Abgr8888 => PixelFormat::Abgr8888,
				DrmFourcc::Xbgr8888 => PixelFormat::Xbgr8888,
				DrmFourcc::Abgr2101010 => PixelFormat::Abgr2101010,
				DrmFourcc::Xbgr2101010 => PixelFormat::Xbgr2101010,
				_ => return Err(anyhow!("Unsupported DRM format from KWin: {:?}", fourcc)),
			}
		} else {
			match format_raw {
				4 => PixelFormat::Xrgb8888,
				5 => PixelFormat::Argb8888,
				6 => PixelFormat::Argb8888,
				16 => PixelFormat::Xbgr8888,
				17 => PixelFormat::Abgr8888,
				18 => PixelFormat::Abgr8888,
				_ => {
					return Err(anyhow!(
						"Unsupported QImage format from KWin: {}",
						format_raw
					));
				}
			}
		};

		let expected_width_bytes = (width * 4) as usize;
		if stride as usize > expected_width_bytes {
			let mut packed_raw = Vec::with_capacity(expected_width_bytes * height as usize);
			for row in raw.chunks_exact(stride as usize) {
				packed_raw.extend_from_slice(&row[..expected_width_bytes]);
			}
			raw = packed_raw;
		}

		convert_to_rgba(&mut raw, pixel_format)
			.ok_or_else(|| anyhow!("Failed to convert pixel format"))?;

		ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, raw)
			.ok_or_else(|| anyhow!("Failed to create image buffer"))
	}
}

impl CaptureBackend for KdeBackend {
	fn get_outputs(&self) -> Result<Vec<OutputInfo>> {
		self.wayland_state.get_outputs()
	}

	fn capture_output(
		&self,
		output: &OutputInfo,
		_region: Option<LogicalRegion>,
		include_cursor: bool,
	) -> Result<RgbaImage> {
		let mut options: arg::PropMap = HashMap::new();
		options.insert(
			"include-cursor".to_string(),
			arg::Variant(Box::new(include_cursor)),
		);

		self.perform_kwin_capture(|proxy, fd| {
			let (dict,): (arg::PropMap,) = proxy.method_call(
				"org.kde.KWin.ScreenShot2",
				"CaptureScreen",
				(output.name.as_str(), options, fd),
			)?;
			Ok(dict)
		})
	}

	fn capture_all_outputs(&self, include_cursor: bool) -> Result<RgbaImage> {
		let mut options: arg::PropMap = HashMap::new();
		options.insert(
			"include-cursor".to_string(),
			arg::Variant(Box::new(include_cursor)),
		);

		self.perform_kwin_capture(|proxy, fd| {
			let (dict,): (arg::PropMap,) = proxy.method_call(
				"org.kde.KWin.ScreenShot2",
				"CaptureWorkspace",
				(options, fd),
			)?;
			Ok(dict)
		})
	}
}
