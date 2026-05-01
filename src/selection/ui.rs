use anyhow::Result;
use image::{GenericImageView, RgbaImage};
use libframr::FramrConnection;
use smithay_client_toolkit::{
	compositor::CompositorState,
	output::OutputState,
	registry::RegistryState,
	seat::SeatState,
	shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell},
	shm::{Shm, slot::SlotPool},
};
use std::sync::{Arc, Mutex};
use wayland_client::{Connection, globals::registry_queue_init};

use crate::config::SelectionConfig;
use crate::selection::backend::wayland::{AppState, SurfaceData};
use crate::selection::graphics;
use crate::selection::state::{SelectionState, Tool};

pub enum UserEvent {
	ProcessingFinished {
		surface_idx: usize,
		blurred_img: RgbaImage,
		pixelated_img: RgbaImage,
	},
}

pub struct SelectionUI {
	outputs: Vec<(libframr::OutputInfo, RgbaImage)>,
	state: Arc<Mutex<SelectionState>>,
}

fn image_to_cairo_surface(img: &RgbaImage) -> cairo::ImageSurface {
	let (w, h) = img.dimensions();
	let stride = w as i32 * 4;
	let mut cairo_data = img.as_raw().clone();
	for pixel in cairo_data.chunks_exact_mut(4) {
		pixel.swap(0, 2); // RGBA to BGRA
	}

	cairo::ImageSurface::create_for_data(
		cairo_data,
		cairo::Format::ARgb32,
		w as i32,
		h as i32,
		stride,
	)
	.expect("failed to create cairo surface")
}

impl SelectionUI {
	pub fn new(config: SelectionConfig) -> Result<Self> {
		let conn = FramrConnection::new()?;
		let outputs_info = conn.get_all_outputs()?;
		let mut outputs = Vec::new();

		for info in outputs_info {
			let img = conn.screenshot_output(&info, true)?;
			outputs.push((info, img));
		}

		let mut last_surface_width = 1920.0;
		if let Some((info, _)) = outputs.first() {
			last_surface_width = info.logical_size.width as f64;
		}

		Ok(Self {
			outputs,
			state: Arc::new(Mutex::new(SelectionState {
				start: None,
				end: None,
				current: (0.0, 0.0),
				is_dragging: false,
				active_tool: Tool::Select,
				annotations: Vec::new(),
				finished: false,
				cancelled: false,
				last_surface_width,
				dirty: true,
				current_offset: (0.0, 0.0),
				editing_text_idx: None,
				config,
			})),
		})
	}

	pub fn run(self) -> Result<Option<RgbaImage>> {
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
		for (info, _) in &self.outputs {
			total_buffer_size += (info.logical_size.width * info.logical_size.height * 4) * 2;
		}
		let pool_size = std::cmp::max(1024 * 1024 * 64, total_buffer_size as usize);
		let pool = SlotPool::new(pool_size, &shm_state)?;

		let (tx, rx) = std::sync::mpsc::channel();

		let mut app = AppState {
			registry_state,
			output_state,
			compositor_state,
			shm_state,
			layer_shell,
			seat_state,
			pool,
			surfaces: Vec::new(),
			state: self.state.clone(),
			exit: false,
			rx,
		};

		event_queue.roundtrip(&mut app)?;

		for (i, (info, img)) in self.outputs.iter().enumerate() {
			let (w, h) = (info.logical_size.width, info.logical_size.height);

			let cached_bg = image_to_cairo_surface(img);
			let cached_blurred_bg = cached_bg.clone();
			let cached_pixelated_bg = cached_bg.clone();

			let wl_surface = app.compositor_state.create_surface(&qh);
			let layer = app.layer_shell.create_layer_surface(
				&qh,
				wl_surface.clone(),
				Layer::Overlay,
				Some("framr-selection"),
				Some(
					&app.output_state
						.outputs()
						.find(|o| {
							let info_name = app.output_state.info(o).and_then(|i| i.name);
							info_name.as_deref() == Some(&info.name)
						})
						.unwrap_or_else(|| app.output_state.outputs().next().unwrap()),
				),
			);

			layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT | Anchor::BOTTOM);
			layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
			layer.set_exclusive_zone(-1);
			layer.set_size(w, h);
			wl_surface.commit();

			app.surfaces.push(SurfaceData {
				output: info.clone(),
				cached_bg,
				cached_blurred_bg,
				cached_pixelated_bg,
				_layer: layer,
				wl_surface,
				dimensions: (w, h),
				slot: None,
			});

			let info = info.clone();
			let img = img.clone();
			let tx = tx.clone();
			let conn_handle = conn.clone();
			let (blur_radius, pixelate_block_size) = {
				let state = app.state.lock().unwrap();
				(state.config.blur_radius, state.config.pixelate_block_size)
			};
			std::thread::spawn(move || {
				let (w, h) = (info.logical_size.width, info.logical_size.height);
				let mut blurred_img = img.clone();
				graphics::apply_blur(&mut blurred_img, 0, 0, w, h, blur_radius);
				let mut pixelated_img = img.clone();
				graphics::apply_pixelate(&mut pixelated_img, 0, 0, w, h, pixelate_block_size);

				let _ = tx.send(UserEvent::ProcessingFinished {
					surface_idx: i,
					blurred_img,
					pixelated_img,
				});
				let _ = conn_handle.flush();
			});
		}

		app.state.lock().unwrap().dirty = true;

		while !app.exit {
			event_queue.blocking_dispatch(&mut app)?;

			while let Ok(event) = app.rx.try_recv() {
				match event {
					UserEvent::ProcessingFinished {
						surface_idx,
						blurred_img,
						pixelated_img,
					} => {
						if let Some(sd) = app.surfaces.get_mut(surface_idx) {
							sd.cached_blurred_bg = image_to_cairo_surface(&blurred_img);
							sd.cached_pixelated_bg = image_to_cairo_surface(&pixelated_img);
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
				for i in 0..app.surfaces.len() {
					if let Err(e) = app.draw(i, &state, &qh) {
						eprintln!("Draw error: {}", e);
					}
				}
				state.dirty = false;
			}
		}

		let state = self.state.lock().unwrap();
		if state.cancelled || state.start.is_none() || state.end.is_none() {
			return Ok(None);
		}

		let (s_x, s_y) = state.start.unwrap();
		let (e_x, e_y) = state.end.unwrap();

		let x = s_x.min(e_x) as i32;
		let y = s_y.min(e_y) as i32;
		let width = (s_x - e_x).abs() as u32;
		let height = (s_y - e_y).abs() as u32;

		if width == 0 || height == 0 {
			return Ok(None);
		}

		let mut final_img = RgbaImage::new(width, height);
		let mut has_content = false;

		for (info, img) in &self.outputs {
			let out_x = info.logical_position.x;
			let out_y = info.logical_position.y;
			let out_w = info.logical_size.width as i32;
			let out_h = info.logical_size.height as i32;

			let intersect_x = x.max(out_x);
			let intersect_y = y.max(out_y);
			let intersect_x2 = (x + width as i32).min(out_x + out_w);
			let intersect_y2 = (y + height as i32).min(out_y + out_h);

			if intersect_x < intersect_x2 && intersect_y < intersect_y2 {
				let mut base = img.clone();
				graphics::apply_annotations(&mut base, &state.annotations, info, &state.config);

				let local_x = (intersect_x - out_x) as u32;
				let local_y = (intersect_y - out_y) as u32;
				let intersect_w = (intersect_x2 - intersect_x) as u32;
				let intersect_h = (intersect_y2 - intersect_y) as u32;

				let target_x = (intersect_x - x) as u32;
				let target_y = (intersect_y - y) as u32;

				let cropped = base
					.view(local_x, local_y, intersect_w, intersect_h)
					.to_image();

				for py in 0..intersect_h {
					for px in 0..intersect_w {
						final_img.put_pixel(
							target_x + px,
							target_y + py,
							*cropped.get_pixel(px, py),
						);
					}
				}
				has_content = true;
			}
		}

		if has_content {
			Ok(Some(final_img))
		} else {
			Ok(None)
		}
	}
}
