use std::ffi::CString;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::ptr;

use anyhow::{Result, anyhow};
use ffmpeg::filter;
use ffmpeg::format::Pixel;
use ffmpeg::util::frame::video::Video;
use ffmpeg_next as ffmpeg;

use crate::{Frame, FrameData, PixelFormat};

pub(crate) struct VaapiFrames {
	device: *mut ffmpeg::ffi::AVBufferRef,
	frames: *mut ffmpeg::ffi::AVBufferRef,
	drm_device: *mut ffmpeg::ffi::AVBufferRef,
	drm_frames: *mut ffmpeg::ffi::AVBufferRef,
	vaapi_source_frames: *mut ffmpeg::ffi::AVBufferRef,
	dmabuf_input: bool,
	graph: filter::Graph,
}

impl VaapiFrames {
	pub(crate) fn new(
		encoder: &mut ffmpeg::encoder::video::Video,
		device_path: Option<&Path>,
		software_format: Pixel,
		width: u32,
		height: u32,
		visible_width: u32,
		visible_height: u32,
		source_format: Pixel,
		source_width: u32,
		source_height: u32,
		fps: u32,
		dmabuf_input: bool,
	) -> Result<Self> {
		let device_path = crate::gpu::render_node(device_path)?;
		let device_path = device_path.as_path();
		let device_name = CString::new(device_path.as_os_str().as_encoded_bytes())?;
		let mut device = ptr::null_mut();
		let create_result = unsafe {
			ffmpeg::ffi::av_hwdevice_ctx_create(
				&mut device,
				ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
				device_name.as_ptr(),
				ptr::null_mut(),
				0,
			)
		};
		if create_result < 0 {
			return Err(anyhow!(
				"failed to open VAAPI device {}: {}",
				device_path.display(),
				ffmpeg::Error::from(create_result)
			));
		}
		let frames = unsafe { ffmpeg::ffi::av_hwframe_ctx_alloc(device) };
		if frames.is_null() {
			unsafe { ffmpeg::ffi::av_buffer_unref(&mut device) };
			return Err(anyhow!("failed to allocate VAAPI frame context"));
		}
		unsafe {
			let context = (*frames).data.cast::<ffmpeg::ffi::AVHWFramesContext>();
			(*context).format = ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_VAAPI;
			(*context).sw_format = software_format.into();
			(*context).width = width as i32;
			(*context).height = height as i32;
			(*context).initial_pool_size = 8;
		}
		let init_result = unsafe { ffmpeg::ffi::av_hwframe_ctx_init(frames) };
		if init_result < 0 {
			let mut frames = frames;
			unsafe {
				ffmpeg::ffi::av_buffer_unref(&mut frames);
				ffmpeg::ffi::av_buffer_unref(&mut device);
			}
			return Err(anyhow!(
				"failed to initialize VAAPI frames: {}",
				ffmpeg::Error::from(init_result)
			));
		}
		unsafe {
			(*encoder.as_mut_ptr()).hw_frames_ctx = ffmpeg::ffi::av_buffer_ref(frames);
		}
		let mut result = Self {
			device,
			frames,
			drm_device: ptr::null_mut(),
			drm_frames: ptr::null_mut(),
			vaapi_source_frames: ptr::null_mut(),
			dmabuf_input,
			graph: filter::Graph::new(),
		};
		if dmabuf_input {
			result.configure_dmabuf_import(
				device_path,
				source_format,
				source_width,
				source_height,
			)?;
		}
		result.configure_vpp(
			source_format,
			source_width,
			source_height,
			software_format,
			width,
			height,
			visible_width,
			visible_height,
			fps,
		)?;
		Ok(result)
	}

	fn configure_dmabuf_import(
		&mut self,
		device_path: &Path,
		source_format: Pixel,
		width: u32,
		height: u32,
	) -> Result<()> {
		let device_name = CString::new(device_path.as_os_str().as_encoded_bytes())?;
		let result = unsafe {
			ffmpeg::ffi::av_hwdevice_ctx_create(
				&mut self.drm_device,
				ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_DRM,
				device_name.as_ptr(),
				ptr::null_mut(),
				0,
			)
		};
		if result < 0 {
			return Err(anyhow!(
				"failed to create DRM device for DMA-BUF import: {}",
				ffmpeg::Error::from(result)
			));
		}
		self.drm_frames = unsafe { ffmpeg::ffi::av_hwframe_ctx_alloc(self.drm_device) };
		if self.drm_frames.is_null() {
			return Err(anyhow!("failed to allocate DRM frame context"));
		}
		unsafe {
			let context = (*self.drm_frames)
				.data
				.cast::<ffmpeg::ffi::AVHWFramesContext>();
			(*context).format = ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_DRM_PRIME;
			(*context).sw_format = source_format.into();
			(*context).width = width as i32;
			(*context).height = height as i32;
		}
		let result = unsafe { ffmpeg::ffi::av_hwframe_ctx_init(self.drm_frames) };
		if result < 0 {
			return Err(anyhow!(
				"failed to initialize DRM frame context: {}",
				ffmpeg::Error::from(result)
			));
		}
		let result = unsafe {
			ffmpeg::ffi::av_hwframe_ctx_create_derived(
				&mut self.vaapi_source_frames,
				ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_VAAPI,
				self.device,
				self.drm_frames,
				ffmpeg::ffi::AV_HWFRAME_MAP_READ as i32,
			)
		};
		if result < 0 {
			return Err(anyhow!(
				"failed to derive VAAPI frames from DMA-BUF: {}",
				ffmpeg::Error::from(result)
			));
		}
		Ok(())
	}

	fn configure_vpp(
		&mut self,
		source_format: Pixel,
		source_width: u32,
		source_height: u32,
		output_format: Pixel,
		output_width: u32,
		output_height: u32,
		visible_width: u32,
		visible_height: u32,
		fps: u32,
	) -> Result<()> {
		let input_filter =
			filter::find("buffer").ok_or_else(|| anyhow!("FFmpeg buffer filter is unavailable"))?;
		let upload_filter = filter::find("hwupload")
			.ok_or_else(|| anyhow!("FFmpeg hwupload filter is unavailable"))?;
		let scale_filter = filter::find("scale_vaapi")
			.ok_or_else(|| anyhow!("FFmpeg scale_vaapi filter is unavailable"))?;
		let output_filter = filter::find("buffersink")
			.ok_or_else(|| anyhow!("FFmpeg buffersink filter is unavailable"))?;
		let source_name = source_format
			.descriptor()
			.ok_or_else(|| anyhow!("unsupported VAAPI source pixel format"))?
			.name();
		let output_name = output_format
			.descriptor()
			.ok_or_else(|| anyhow!("unsupported VAAPI output pixel format"))?
			.name();
		let input_args = format!(
			"video_size={source_width}x{source_height}:pix_fmt={}:time_base=1/{fps}:pixel_aspect=1/1",
			source_name
		);
		let scale_args = format!(
			"w={visible_width}:h={visible_height}:format={}",
			output_name
		);
		let mut input = self.graph.add(&input_filter, "in", &input_args)?;
		let mut scale = self.add_hardware_filter(&scale_filter, "scale", &scale_args)?;
		let mut output = self.graph.add(&output_filter, "out", "")?;
		if self.dmabuf_input {
			self.set_dmabuf_source_parameters(
				&mut input,
				Pixel::VAAPI,
				source_width,
				source_height,
				fps,
			)?;
			input.link(0, &mut scale, 0);
		} else {
			let mut upload = self.add_hardware_filter(&upload_filter, "upload", "")?;
			input.link(0, &mut upload, 0);
			upload.link(0, &mut scale, 0);
		}
		if output_width != visible_width || output_height != visible_height {
			let pad_filter = filter::find("pad_vaapi")
				.ok_or_else(|| anyhow!("FFmpeg VAAPI padding filter is unavailable"))?;
			let pad_args = format!("w={output_width}:h={output_height}:x=0:y=0:color=black");
			let mut pad = self.add_hardware_filter(&pad_filter, "pad", &pad_args)?;
			scale.link(0, &mut pad, 0);
			pad.link(0, &mut output, 0);
		} else {
			scale.link(0, &mut output, 0);
		}
		self.graph.validate()?;
		Ok(())
	}

	fn set_dmabuf_source_parameters(
		&self,
		input: &mut filter::Context,
		format: Pixel,
		width: u32,
		height: u32,
		fps: u32,
	) -> Result<()> {
		let parameters = unsafe { ffmpeg::ffi::av_buffersrc_parameters_alloc() };
		if parameters.is_null() {
			return Err(anyhow!("failed to allocate DMA-BUF filter parameters"));
		}
		unsafe {
			(*parameters).format = ffmpeg::ffi::AVPixelFormat::from(format) as i32;
			(*parameters).time_base = ffmpeg::ffi::AVRational {
				num: 1,
				den: fps as i32,
			};
			(*parameters).width = width as i32;
			(*parameters).height = height as i32;
			(*parameters).sample_aspect_ratio = ffmpeg::ffi::AVRational { num: 1, den: 1 };
			(*parameters).frame_rate = ffmpeg::ffi::AVRational {
				num: fps as i32,
				den: 1,
			};
			(*parameters).hw_frames_ctx = ffmpeg::ffi::av_buffer_ref(self.vaapi_source_frames);
		}
		let result =
			unsafe { ffmpeg::ffi::av_buffersrc_parameters_set(input.as_mut_ptr(), parameters) };
		unsafe {
			ffmpeg::ffi::av_buffer_unref(&mut (*parameters).hw_frames_ctx);
			ffmpeg::ffi::av_free(parameters.cast());
		}
		if result < 0 {
			return Err(ffmpeg::Error::from(result).into());
		}
		Ok(())
	}

	fn add_hardware_filter(
		&mut self,
		filter: &filter::Filter,
		name: &str,
		args: &str,
	) -> Result<filter::Context> {
		let name = CString::new(name)?;
		let args = CString::new(args)?;
		let context = unsafe {
			ffmpeg::ffi::avfilter_graph_alloc_filter(
				self.graph.as_mut_ptr(),
				filter.as_ptr(),
				name.as_ptr(),
			)
		};
		if context.is_null() {
			return Err(anyhow!("failed to allocate VAAPI video filter"));
		}
		let device = unsafe { ffmpeg::ffi::av_buffer_ref(self.device) };
		if device.is_null() {
			return Err(anyhow!(
				"failed to reference VAAPI device for video processing"
			));
		}
		unsafe {
			(*context).hw_device_ctx = device;
		}
		let result = unsafe { ffmpeg::ffi::avfilter_init_str(context, args.as_ptr()) };
		if result < 0 {
			return Err(ffmpeg::Error::from(result).into());
		}
		Ok(unsafe { filter::Context::wrap(context) })
	}

	pub(crate) fn uses_dmabuf(&self) -> bool {
		self.dmabuf_input
	}

	pub(crate) fn import_dmabuf(&self, frame: &Frame, pts: i64) -> Result<Video> {
		let FrameData::DmaBuf(dmabuf) = &frame.data else {
			return Err(anyhow!("VAAPI DMA-BUF input received a memory frame"));
		};
		if dmabuf.planes.is_empty() || dmabuf.planes.len() > 4 {
			return Err(anyhow!("unsupported DMA-BUF plane count"));
		}
		let height = usize::try_from(frame.format.height)?;
		let object_sizes = dmabuf
			.planes
			.iter()
			.map(|plane| {
				(plane.offset as usize)
					.checked_add(
						(plane.stride as usize)
							.checked_mul(height)
							.ok_or_else(|| anyhow!("DMA-BUF plane is too large"))?,
					)
					.ok_or_else(|| anyhow!("DMA-BUF plane is too large"))
			})
			.collect::<Result<Vec<_>>>()?;
		let descriptor = unsafe {
			ffmpeg::ffi::av_mallocz(std::mem::size_of::<ffmpeg::ffi::AVDRMFrameDescriptor>())
				.cast::<ffmpeg::ffi::AVDRMFrameDescriptor>()
		};
		if descriptor.is_null() {
			return Err(anyhow!("failed to allocate DRM frame descriptor"));
		}
		unsafe {
			(*descriptor).nb_objects = dmabuf.planes.len() as i32;
			(*descriptor).nb_layers = 1;
			(*descriptor).layers[0].format = drm_format(frame.format.format);
			(*descriptor).layers[0].nb_planes = dmabuf.planes.len() as i32;
			for (index, plane) in dmabuf.planes.iter().enumerate() {
				(*descriptor).objects[index].fd = plane.fd.as_raw_fd();
				(*descriptor).objects[index].size = object_sizes[index];
				(*descriptor).objects[index].format_modifier = dmabuf.modifier;
				(*descriptor).layers[0].planes[index].object_index = index as i32;
				(*descriptor).layers[0].planes[index].offset = plane.offset as isize;
				(*descriptor).layers[0].planes[index].pitch = plane.stride as isize;
			}
		}
		let buffer = unsafe {
			ffmpeg::ffi::av_buffer_create(
				descriptor.cast(),
				std::mem::size_of::<ffmpeg::ffi::AVDRMFrameDescriptor>(),
				Some(ffmpeg::ffi::av_buffer_default_free),
				ptr::null_mut(),
				0,
			)
		};
		if buffer.is_null() {
			unsafe { ffmpeg::ffi::av_free(descriptor.cast()) };
			return Err(anyhow!("failed to reference DRM frame descriptor"));
		}
		let mut drm_frame = Video::empty();
		unsafe {
			let raw = drm_frame.as_mut_ptr();
			(*raw).format = ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_DRM_PRIME as i32;
			(*raw).width = frame.format.width;
			(*raw).height = frame.format.height;
			(*raw).pts = pts;
			(*raw).data[0] = descriptor.cast();
			(*raw).buf[0] = buffer;
			(*raw).hw_frames_ctx = ffmpeg::ffi::av_buffer_ref(self.drm_frames);
		}
		let mut vaapi_frame = Video::empty();
		unsafe {
			let raw = vaapi_frame.as_mut_ptr();
			(*raw).format = ffmpeg::ffi::AVPixelFormat::AV_PIX_FMT_VAAPI as i32;
			(*raw).hw_frames_ctx = ffmpeg::ffi::av_buffer_ref(self.vaapi_source_frames);
		}
		let result = unsafe {
			ffmpeg::ffi::av_hwframe_map(
				vaapi_frame.as_mut_ptr(),
				drm_frame.as_ptr(),
				ffmpeg::ffi::AV_HWFRAME_MAP_READ as i32,
			)
		};
		if result < 0 {
			return Err(anyhow!(
				"failed to import DMA-BUF into VAAPI: {}",
				ffmpeg::Error::from(result)
			));
		}
		vaapi_frame.set_pts(Some(pts));
		Ok(vaapi_frame)
	}

	pub(crate) fn process(&mut self, source: &Video) -> Result<Video> {
		self.graph
			.get("in")
			.ok_or_else(|| anyhow!("VAAPI input filter disappeared"))?
			.source()
			.add(source)?;
		let mut output = Video::empty();
		self.graph
			.get("out")
			.ok_or_else(|| anyhow!("VAAPI output filter disappeared"))?
			.sink()
			.frame(&mut output)?;
		Ok(output)
	}
}

impl Drop for VaapiFrames {
	fn drop(&mut self) {
		unsafe {
			ffmpeg::ffi::av_buffer_unref(&mut self.vaapi_source_frames);
			ffmpeg::ffi::av_buffer_unref(&mut self.drm_frames);
			ffmpeg::ffi::av_buffer_unref(&mut self.drm_device);
			ffmpeg::ffi::av_buffer_unref(&mut self.frames);
			ffmpeg::ffi::av_buffer_unref(&mut self.device);
		}
	}
}

fn drm_format(format: PixelFormat) -> u32 {
	use drm_fourcc::DrmFourcc;
	match format {
		PixelFormat::Argb8888 => DrmFourcc::Argb8888 as u32,
		PixelFormat::Xrgb8888 => DrmFourcc::Xrgb8888 as u32,
		PixelFormat::Abgr8888 => DrmFourcc::Abgr8888 as u32,
		PixelFormat::Xbgr8888 => DrmFourcc::Xbgr8888 as u32,
		PixelFormat::Abgr2101010 => DrmFourcc::Abgr2101010 as u32,
		PixelFormat::Xbgr2101010 => DrmFourcc::Xbgr2101010 as u32,
	}
}
