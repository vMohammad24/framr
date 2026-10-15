use anyhow::Result;
use image::{GenericImageView, RgbaImage, imageops::FilterType};
use libframr::{FramrConnection, LogicalRegion, Position, Size};
use smithay_client_toolkit::{
	compositor::CompositorState,
	output::OutputState,
	registry::RegistryState,
	seat::SeatState,
	seat::pointer::cursor_shape::CursorShapeManager,
	shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell},
	shm::{Shm, slot::SlotPool},
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use wayland_client::{Connection, globals::registry_queue_init};

use crate::config::SelectionConfig;
use crate::selection::backend::wayland::{AppState, SurfaceData, prerender_toolbar_icons};
use crate::selection::graphics;
use crate::selection::state::{BgraImage, SelectionState, Tool};
use crate::selection::window::{Window, get_windows};

pub enum UserEvent {
	BlurFinished {
		surface_idx: usize,
		blurred_img: BgraImage,
	},
	PixelateFinished {
		surface_idx: usize,
		pixelated_img: BgraImage,
	},
}

pub struct SelectionUI {
	outputs: Arc<Vec<(libframr::OutputInfo, BgraImage)>>,
	prerendered_toolbar: Option<(Vec<u8>, i32, i32)>,
	state: Arc<Mutex<SelectionState>>,
}

fn bgra_image_to_cairo_surface(img: BgraImage) -> Result<cairo::ImageSurface> {
	let mut img = img.0;
	let (w, h) = img.dimensions();
	let stride = w as i32 * 4;
	for px in img.as_mut().chunks_exact_mut(4) {
		px[3] = 255;
	}

	cairo::ImageSurface::create_for_data(
		img.into_raw(),
		cairo::Format::ARgb32,
		w as i32,
		h as i32,
		stride,
	)
	.map_err(|e| anyhow::anyhow!("failed to create cairo surface: {e}"))
}

impl SelectionUI {
	pub fn new(config: SelectionConfig) -> Result<Self> {
		let windows_handle = std::thread::spawn(get_windows);
		let toolbar_cfg = config;
		let toolbar_handle = std::thread::spawn(move || prerender_toolbar_icons(&toolbar_cfg));

		let conn = FramrConnection::new()?;
		let outputs = conn.screenshot_each_output_bgra(true)?;

		let mut last_surface_width = 1920.0;
		let mut initial_offset = (0.0, 0.0);
		if let Some((info, _)) = outputs.first() {
			last_surface_width = info.logical_size.width as f64;
			initial_offset = (
				info.logical_position.x as f64,
				info.logical_position.y as f64,
			);
		}

		let windows = windows_handle
			.join()
			.ok()
			.and_then(Result::ok)
			.unwrap_or_default();
		let prerendered_toolbar = toolbar_handle.join().ok().flatten();
		let outputs = Arc::new(outputs);

		Ok(Self {
			outputs: outputs.clone(),
			prerendered_toolbar,
			state: Arc::new(Mutex::new(SelectionState {
				start: None,
				end: None,
				regions: Vec::new(),
				selected_region: None,
				region_interaction: None,
				original_region: None,
				current: (0.0, 0.0),
				pointer_modifiers: (false, false),
				is_dragging: false,
				active_tool: Tool::Select,
				annotations: Vec::new(),
				source_images: outputs,
				undo_stack: VecDeque::new(),
				redo_stack: VecDeque::new(),
				pending_annotation_history: None,
				pending_region_history: None,
				selected_annotation: None,
				is_moving_annotation: false,
				move_start_point: None,
				annotation_draw_origin: None,
				annotation_resize_index: None,
				annotation_resize_handle: None,
				original_annotation: None,
				annotation_move_delta: (0.0, 0.0),
				finished: false,
				cancelled: false,
				last_surface_width,
				dirty: true,
				current_offset: initial_offset,
				editing_text_idx: None,
				config,
				windows,
				hovered_window: None,
			})),
		})
	}

	pub fn run(
		mut self,
		capture_image: bool,
	) -> Result<Option<(LogicalRegion, Option<RgbaImage>, Vec<Window>)>> {
		let conn = Connection::connect_to_env()?;
		let (globals, mut event_queue) = registry_queue_init(&conn)?;
		let qh = event_queue.handle();

		let registry_state = RegistryState::new(&globals);
		let output_state = OutputState::new(&globals, &qh);
		let compositor_state = CompositorState::bind(&globals, &qh)?;
		let shm_state = Shm::bind(&globals, &qh)?;
		let layer_shell = LayerShell::bind(&globals, &qh)?;
		let seat_state = SeatState::new(&globals, &qh);
		let mut total_buffer_size = 0;
		for (info, _) in self.outputs.iter() {
			total_buffer_size += info.logical_size.width * info.logical_size.height * 4;
		}
		let pool_size = std::cmp::max(1024 * 1024, total_buffer_size as usize);
		let pool = SlotPool::new(pool_size, &shm_state)?;

		let (tx, rx) = std::sync::mpsc::channel();

		let cursor_shape_manager = CursorShapeManager::bind(&globals, &qh).ok();

		let mut app = AppState {
			registry_state,
			output_state,
			compositor_state,
			shm_state,
			layer_shell,
			seat_state,
			pool,
			cursor_shape_manager,
			surfaces: Vec::new(),
			state: self.state.clone(),
			exit: false,
			rx,
			modifiers: Default::default(),
			cursor_shape_device: None,
		};

		event_queue.roundtrip(&mut app)?;

		let num_outputs = self.outputs.len();
		let mut toolbar_template = self.prerendered_toolbar.take();

		for (idx, (info, _)) in self.outputs.iter().enumerate() {
			let (w, h) = (info.logical_size.width, info.logical_size.height);
			let wl_output = app
				.output_state
				.outputs()
				.find(|o| {
					let info_name = app.output_state.info(o).and_then(|i| i.name);
					info_name.as_deref() == Some(&info.name)
				})
				.or_else(|| app.output_state.outputs().next())
				.ok_or_else(|| anyhow::anyhow!("compositor reported no outputs"))?;

			let wl_surface = app.compositor_state.create_surface(&qh);
			let layer = app.layer_shell.create_layer_surface(
				&qh,
				wl_surface.clone(),
				Layer::Overlay,
				Some("framr-selection"),
				Some(&wl_output),
			);

			layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT | Anchor::BOTTOM);
			layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
			layer.set_exclusive_zone(-1);
			layer.set_size(w, h);
			wl_surface.commit();

			let cached_toolbar_icons = match toolbar_template {
				Some((ref buf, iw, ih)) if idx + 1 < num_outputs => {
					cairo::ImageSurface::create_for_data(
						buf.clone(),
						cairo::Format::ARgb32,
						iw,
						ih,
						iw * 4,
					)
					.ok()
				}
				Some((buf, iw, ih)) => {
					toolbar_template = None;
					cairo::ImageSurface::create_for_data(buf, cairo::Format::ARgb32, iw, ih, iw * 4)
						.ok()
				}
				None => None,
			};

			app.surfaces.push(SurfaceData {
				output: info.clone(),
				cached_toolbar_icons,
				cached_blurred_bg: None,
				cached_pixelated_bg: None,
				_layer: layer,
				wl_surface,
				dimensions: (w, h),
				slot: None,
				configured: false,
				canvas_ready: false,
				waiting_for_frame: false,
				redraw_pending: true,
			});
		}

		let _ = conn.flush();
		{
			let mut state = self.state.lock().unwrap();
			state.dirty = false;
			for i in 0..app.surfaces.len() {
				if let Err(e) = app.render_canvas(i, &state) {
					eprintln!("Initial render error: {}", e);
				}
			}
		}

		let mut blur_spawned = false;
		let mut pixelate_spawned = false;

		while !app.exit {
			event_queue.blocking_dispatch(&mut app)?;

			if !blur_spawned || !pixelate_spawned {
				let (active_tool, blur_radius, pixelate_block_size) = {
					let state = app.state.lock().unwrap();
					(
						state.active_tool,
						state.config.blur_radius,
						state.config.pixelate_block_size,
					)
				};
				if !pixelate_spawned && active_tool == Tool::Pixelate {
					pixelate_spawned = true;
					for (i, (info, img)) in self.outputs.iter().enumerate() {
						let (w, h) = (info.logical_size.width, info.logical_size.height);
						let outputs = self.outputs.clone();
						let tx = tx.clone();
						let conn_handle = conn.clone();
						let _ = img;
						std::thread::spawn(move || {
							let src = &outputs[i].1.0;
							let mut pixelated_img = if src.width() != w || src.height() != h {
								image::imageops::resize(src, w, h, FilterType::Nearest)
							} else {
								src.clone()
							};
							graphics::apply_pixelate(
								&mut pixelated_img,
								0,
								0,
								w,
								h,
								pixelate_block_size,
							);
							let _ = tx.send(UserEvent::PixelateFinished {
								surface_idx: i,
								pixelated_img: BgraImage(pixelated_img),
							});
							let _ = conn_handle.flush();
						});
					}
				}
				if !blur_spawned && active_tool == Tool::Blur {
					blur_spawned = true;
					for (i, (info, img)) in self.outputs.iter().enumerate() {
						let (w, h) = (info.logical_size.width, info.logical_size.height);
						let outputs = self.outputs.clone();
						let tx = tx.clone();
						let conn_handle = conn.clone();
						let _ = img;
						std::thread::spawn(move || {
							let src = &outputs[i].1.0;
							let mut blurred_img = if src.width() != w || src.height() != h {
								image::imageops::resize(src, w, h, FilterType::Nearest)
							} else {
								src.clone()
							};
							graphics::apply_blur(&mut blurred_img, 0, 0, w, h, blur_radius);
							let _ = tx.send(UserEvent::BlurFinished {
								surface_idx: i,
								blurred_img: BgraImage(blurred_img),
							});
							let _ = conn_handle.flush();
						});
					}
				}
			}

			while let Ok(event) = app.rx.try_recv() {
				match event {
					UserEvent::BlurFinished {
						surface_idx,
						blurred_img,
					} => {
						if let Some(sd) = app.surfaces.get_mut(surface_idx) {
							sd.cached_blurred_bg = Some(bgra_image_to_cairo_surface(blurred_img)?);
							app.state.lock().unwrap().dirty = true;
						}
					}
					UserEvent::PixelateFinished {
						surface_idx,
						pixelated_img,
					} => {
						if let Some(sd) = app.surfaces.get_mut(surface_idx) {
							sd.cached_pixelated_bg =
								Some(bgra_image_to_cairo_surface(pixelated_img)?);
							app.state.lock().unwrap().dirty = true;
						}
					}
				}
			}

			let state_arc = app.state.clone();
			let mut state = state_arc.lock().unwrap();

			if state.finished || state.cancelled {
				app.exit = true;
			}

			if state.dirty {
				for surface in &mut app.surfaces {
					surface.redraw_pending = true;
					surface.canvas_ready = false;
				}
				state.dirty = false;
			}

			for i in 0..app.surfaces.len() {
				if app.surfaces[i].redraw_pending
					&& app.surfaces[i].configured
					&& !app.surfaces[i].waiting_for_frame
				{
					match app.draw(i, &state, &qh) {
						Ok(true) => {}
						Ok(false) => {}
						Err(e) => eprintln!("Draw error: {}", e),
					}
				}
			}
		}

		let mut state = self.state.lock().unwrap();
		if state.cancelled {
			return Ok(None);
		}

		let Some(bounds) = state.selection_bounds() else {
			return Ok(None);
		};

		let x = bounds.start.0.floor() as i32;
		let y = bounds.start.1.floor() as i32;
		let right = bounds.end.0.ceil() as i32;
		let bottom = bounds.end.1.ceil() as i32;
		let width = (right - x).max(0) as u32;
		let height = (bottom - y).max(0) as u32;

		if width == 0 || height == 0 {
			return Ok(None);
		}

		let region = LogicalRegion {
			position: Position { x, y },
			size: Size { width, height },
		};

		let windows = std::mem::take(&mut state.windows);

		if !capture_image {
			return Ok(Some((region, None, windows)));
		}

		let mut max_scale_x = 1.0_f64;
		let mut max_scale_y = 1.0_f64;
		for (info, src_img) in self.outputs.iter() {
			let out_x = info.logical_position.x;
			let out_y = info.logical_position.y;
			let out_w = info.logical_size.width as i32;
			let out_h = info.logical_size.height as i32;
			let intersects = state.regions.iter().any(|selected| {
				let region_x = selected.start.0.floor() as i32;
				let region_y = selected.start.1.floor() as i32;
				let region_right = selected.end.0.ceil() as i32;
				let region_bottom = selected.end.1.ceil() as i32;
				region_x.max(out_x) < region_right.min(out_x + out_w)
					&& region_y.max(out_y) < region_bottom.min(out_y + out_h)
			});
			if intersects {
				let sx = src_img.0.width() as f64 / info.logical_size.width.max(1) as f64;
				let sy = src_img.0.height() as f64 / info.logical_size.height.max(1) as f64;
				max_scale_x = max_scale_x.max(sx);
				max_scale_y = max_scale_y.max(sy);
			}
		}

		let final_w = ((width as f64 * max_scale_x).round() as u32).max(1);
		let final_h = ((height as f64 * max_scale_y).round() as u32).max(1);
		let mut final_img = RgbaImage::new(final_w, final_h);
		let mut has_content = false;

		for (info, src_bgra) in self.outputs.iter() {
			let out_x = info.logical_position.x;
			let out_y = info.logical_position.y;
			let out_w = info.logical_size.width as i32;
			let out_h = info.logical_size.height as i32;

			let output_intersects_any = state.regions.iter().any(|selected| {
				let region_x = selected.start.0.floor() as i32;
				let region_y = selected.start.1.floor() as i32;
				let region_right = selected.end.0.ceil() as i32;
				let region_bottom = selected.end.1.ceil() as i32;
				region_x.max(out_x) < region_right.min(out_x + out_w)
					&& region_y.max(out_y) < region_bottom.min(out_y + out_h)
			});
			if !output_intersects_any {
				continue;
			}

			let src_w = src_bgra.0.width();
			let src_h = src_bgra.0.height();
			let scale_x = src_w as f64 / info.logical_size.width.max(1) as f64;
			let scale_y = src_h as f64 / info.logical_size.height.max(1) as f64;

			if state.annotations.is_empty()
				&& (scale_x - max_scale_x).abs() < 1e-4
				&& (scale_y - max_scale_y).abs() < 1e-4
			{
				let src_raw = src_bgra.0.as_raw();
				let src_stride = (src_w as usize) * 4;
				let dst_stride = (final_w as usize) * 4;
				let dst_raw = final_img.as_mut();

				for selected in &state.regions {
					let region_x = selected.start.0.floor() as i32;
					let region_y = selected.start.1.floor() as i32;
					let region_right = selected.end.0.ceil() as i32;
					let region_bottom = selected.end.1.ceil() as i32;
					let intersect_x = region_x.max(out_x);
					let intersect_y = region_y.max(out_y);
					let intersect_x2 = region_right.min(out_x + out_w);
					let intersect_y2 = region_bottom.min(out_y + out_h);

					if intersect_x < intersect_x2 && intersect_y < intersect_y2 {
						let local_x = (((intersect_x - out_x) as f64 * scale_x).round() as usize)
							.min(src_w as usize);
						let local_y = (((intersect_y - out_y) as f64 * scale_y).round() as usize)
							.min(src_h as usize);
						let target_x = (((intersect_x - x) as f64 * max_scale_x).round() as usize)
							.min(final_w as usize);
						let target_y = (((intersect_y - y) as f64 * max_scale_y).round() as usize)
							.min(final_h as usize);
						let phys_w = (((intersect_x2 - intersect_x) as f64 * scale_x).round()
							as usize)
							.min((src_w as usize).saturating_sub(local_x))
							.min((final_w as usize).saturating_sub(target_x));
						let phys_h = (((intersect_y2 - intersect_y) as f64 * scale_y).round()
							as usize)
							.min((src_h as usize).saturating_sub(local_y))
							.min((final_h as usize).saturating_sub(target_y));
						let row_bytes = phys_w * 4;

						for row in 0..phys_h {
							let s_start = (local_y + row) * src_stride + local_x * 4;
							let d_start = (target_y + row) * dst_stride + target_x * 4;
							let s_slice = &src_raw[s_start..s_start + row_bytes];
							let d_slice = &mut dst_raw[d_start..d_start + row_bytes];
							for (dst_px, src_px) in
								d_slice.chunks_exact_mut(4).zip(s_slice.chunks_exact(4))
							{
								dst_px[0] = src_px[2];
								dst_px[1] = src_px[1];
								dst_px[2] = src_px[0];
								dst_px[3] = 255;
							}
						}
						has_content = true;
					}
				}
			} else {
				let mut base = src_bgra.0.clone();
				for pixel in base.as_mut().chunks_exact_mut(4) {
					pixel.swap(0, 2); // BGRA to RGBA
					pixel[3] = 255;
				}
				graphics::apply_annotations(&mut base, &state.annotations, info, &state.config)?;

				for selected in &state.regions {
					let region_x = selected.start.0.floor() as i32;
					let region_y = selected.start.1.floor() as i32;
					let region_right = selected.end.0.ceil() as i32;
					let region_bottom = selected.end.1.ceil() as i32;
					let intersect_x = region_x.max(out_x);
					let intersect_y = region_y.max(out_y);
					let intersect_x2 = region_right.min(out_x + out_w);
					let intersect_y2 = region_bottom.min(out_y + out_h);

					if intersect_x < intersect_x2 && intersect_y < intersect_y2 {
						let local_x = (((intersect_x - out_x) as f64 * scale_x).round() as u32)
							.min(base.width());
						let local_y = (((intersect_y - out_y) as f64 * scale_y).round() as u32)
							.min(base.height());
						let intersect_w = (((intersect_x2 - intersect_x) as f64 * scale_x).round()
							as u32)
							.min(base.width().saturating_sub(local_x));
						let intersect_h = (((intersect_y2 - intersect_y) as f64 * scale_y).round()
							as u32)
							.min(base.height().saturating_sub(local_y));
						let target_x = ((intersect_x - x) as f64 * max_scale_x).round() as i64;
						let target_y = ((intersect_y - y) as f64 * max_scale_y).round() as i64;

						if intersect_w > 0 && intersect_h > 0 {
							let view = base.view(local_x, local_y, intersect_w, intersect_h);
							if (scale_x - max_scale_x).abs() < 1e-4
								&& (scale_y - max_scale_y).abs() < 1e-4
							{
								image::imageops::replace(
									&mut final_img,
									&*view,
									target_x,
									target_y,
								);
							} else {
								let scaled_w = (((intersect_x2 - intersect_x) as f64 * max_scale_x)
									.round() as u32)
									.max(1);
								let scaled_h = (((intersect_y2 - intersect_y) as f64 * max_scale_y)
									.round() as u32)
									.max(1);
								let resized = image::imageops::resize(
									&*view,
									scaled_w,
									scaled_h,
									FilterType::Triangle,
								);
								image::imageops::replace(
									&mut final_img,
									&resized,
									target_x,
									target_y,
								);
							}
							has_content = true;
						}
					}
				}
			}
		}

		if has_content {
			Ok(Some((region, Some(final_img), windows)))
		} else {
			Ok(None)
		}
	}
}
