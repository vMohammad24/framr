use anyhow::{Result, anyhow};
use cairo::{Context, Format, ImageSurface};
use libframr::{OutputInfo, Position};
use pangocairo::functions::{create_layout, show_layout};
use smithay_client_toolkit::{
	delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
	delegate_registry, delegate_seat, delegate_shm,
	output::{OutputHandler, OutputState},
	registry::{ProvidesRegistryState, RegistryHandler, RegistryState},
	seat::{
		Capability, SeatHandler, SeatState,
		keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
		pointer::cursor_shape::CursorShapeManager,
		pointer::{PointerEvent, PointerEventKind, PointerHandler},
	},
	shell::{
		WaylandSurface,
		wlr_layer::{LayerShell, LayerShellHandler, LayerSurface},
	},
	shm::{
		Shm, ShmHandler,
		slot::{Buffer, SlotPool},
	},
};
use std::sync::{Arc, Mutex};
use wayland_client::protocol::{
	wl_keyboard::{self, WlKeyboard},
	wl_output, wl_pointer, wl_seat, wl_shm,
	wl_surface::{self, WlSurface},
};
use wayland_client::{Connection, QueueHandle};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
	Shape, WpCursorShapeDeviceV1,
};

use crate::selection::{graphics, state::{SelectionState, Tool}};
use crate::config::SelectionConfig;

pub struct SurfaceData {
	pub output: OutputInfo,
	pub cached_bg: ImageSurface,
	pub cached_blurred_bg: ImageSurface,
	pub cached_pixelated_bg: ImageSurface,
	pub _layer: LayerSurface,
	pub wl_surface: WlSurface,
	pub dimensions: (u32, u32),
	pub slot: Option<Buffer>,
}

pub struct AppState {
	pub registry_state: RegistryState,
	pub output_state: OutputState,
	pub compositor_state: smithay_client_toolkit::compositor::CompositorState,
	pub shm_state: Shm,
	pub layer_shell: LayerShell,
	pub seat_state: SeatState,
	pub pool: SlotPool,
	pub cursor_shape_manager: Option<CursorShapeManager>,

	pub surfaces: Vec<SurfaceData>,
	pub state: Arc<Mutex<SelectionState>>,

	pub rx: std::sync::mpsc::Receiver<crate::selection::ui::UserEvent>,

	pub exit: bool,
	pub modifiers: Modifiers,
	pub cursor_shape_device: Option<WpCursorShapeDeviceV1>,
}

impl AppState {
	pub fn draw(
		&mut self,
		surface_index: usize,
		state: &SelectionState,
		_: &QueueHandle<Self>,
	) -> Result<()> {
		let surface_data = &mut self.surfaces[surface_index];
		let (width, height) = surface_data.dimensions;
		let stride = width as i32 * 4;

		let (buffer, canvas) = self
			.pool
			.create_buffer(
				width as i32,
				height as i32,
				stride,
				wl_shm::Format::Xrgb8888,
			)
			.map_err(|e| anyhow!("failed to create buffer: {}", e))?;

		let mut cairo_surface = ImageSurface::create(Format::ARgb32, width as i32, height as i32)
			.map_err(|e| anyhow!("failed to create cairo surface: {}", e))?;
		{
			let cr = Context::new(&cairo_surface)
				.map_err(|e| anyhow!("failed to create context: {}", e))?;

			if let Err(e) = cr.set_source_surface(&surface_data.cached_bg, 0.0, 0.0) {
				eprintln!("failed to set source surface: {}", e);
			}
			if let Err(e) = cr.paint() {
				eprintln!("failed to paint: {}", e);
			}

			for (idx, ann) in state.annotations.iter().enumerate() {
				if ann.tool == Tool::Blur || ann.tool == Tool::Pixelate {
					if ann.points.len() >= 2 {
						let offset_x = surface_data.output.logical_position.x as f64;
						let offset_y = surface_data.output.logical_position.y as f64;
						let x1 = ann.points[0].0 - offset_x;
						let y1 = ann.points[0].1 - offset_y;
						let x2 = ann.points[1].0 - offset_x;
						let y2 = ann.points[1].1 - offset_y;

						let x = x1.min(x2);
						let y = y1.min(y2);
						let w = (x1 - x2).abs();
						let h = (y1 - y2).abs();

						if w > 0.0 && h > 0.0 {
							if ann.tool == Tool::Blur {
								if let Err(e) =
									cr.set_source_surface(&surface_data.cached_blurred_bg, 0.0, 0.0)
								{
									eprintln!("failed to set blurred source surface: {}", e);
								}
							} else {
								if let Err(e) = cr.set_source_surface(
									&surface_data.cached_pixelated_bg,
									0.0,
									0.0,
								) {
									eprintln!("failed to set pixelated source surface: {}", e);
								}
							}
							cr.rectangle(x, y, w, h);
							if let Err(e) = cr.fill() {
								eprintln!("failed to fill: {}", e);
							}
						}
					}
				} else {
					graphics::draw_annotation(&cr, ann, &surface_data.output, &state.config);
				}

				if Some(idx) == state.selected_annotation {
					let offset_x = surface_data.output.logical_position.x as f64;
					let offset_y = surface_data.output.logical_position.y as f64;
					cr.set_source_rgba(1.0, 1.0, 1.0, 0.5);
					cr.set_dash(&[5.0, 5.0], 0.0);
					cr.set_line_width(1.0);

					if ann.tool == Tool::Circle && ann.points.len() >= 2 {
						let center = (ann.points[0].0 - offset_x, ann.points[0].1 - offset_y);
						let edge = (ann.points[1].0 - offset_x, ann.points[1].1 - offset_y);
						let radius =
							((center.0 - edge.0).powi(2) + (center.1 - edge.1).powi(2)).sqrt();
						cr.arc(
							center.0,
							center.1,
							radius + 2.0,
							0.0,
							2.0 * std::f64::consts::PI,
						);
						cr.stroke().ok();
					} else if !ann.points.is_empty() {
						let mut min_x = ann.points[0].0;
						let mut min_y = ann.points[0].1;
						let mut max_x = ann.points[0].0;
						let mut max_y = ann.points[0].1;
						for p in &ann.points[1..] {
							min_x = min_x.min(p.0);
							min_y = min_y.min(p.1);
							max_x = max_x.max(p.0);
							max_y = max_y.max(p.1);
						}
						cr.rectangle(
							min_x - offset_x - 5.0,
							min_y - offset_y - 5.0,
							max_x - min_x + 10.0,
							max_y - min_y + 10.0,
						);
						cr.stroke().ok();
					}
					cr.set_dash(&[], 0.0);
				}
			}

			graphics::set_source_color(&cr, state.config.background_color);
			cr.rectangle(0.0, 0.0, width as f64, height as f64);

			if let Some(start) = state.start {
				let current = if state.is_dragging && state.active_tool == Tool::Select {
					state.current
				} else {
					state.end.unwrap_or(state.current)
				};

				let offset_x = surface_data.output.logical_position.x as f64;
				let offset_y = surface_data.output.logical_position.y as f64;
				let s_x = start.0 - offset_x;
				let s_y = start.1 - offset_y;
				let c_x = current.0 - offset_x;
				let c_y = current.1 - offset_y;
				let x = s_x.min(c_x);
				let y = s_y.min(c_y);
				let w = (s_x - c_x).abs();
				let h = (s_y - c_y).abs();

				if w > 0.0 && h > 0.0 {
					cr.rectangle(x, y, w, h);
					cr.set_fill_rule(cairo::FillRule::EvenOdd);
					if let Err(e) = cr.fill() {
						eprintln!("failed to fill selection: {}", e);
					}
					cr.set_fill_rule(cairo::FillRule::Winding);

					graphics::set_source_color(&cr, state.config.border_color);
					cr.set_line_width(state.config.border_width);
					cr.rectangle(x, y, w, h);
					if let Err(e) = cr.stroke() {
						eprintln!("failed to stroke selection: {}", e);
					}

					let dim_text = format!("{}x{}", w as u32, h as u32);
					let layout = create_layout(&cr);
					layout.set_text(&dim_text);
					cr.set_source_rgb(1.0, 1.0, 1.0);
					cr.move_to(x, y - 20.0);
					show_layout(&cr, &layout);
				} else {
					if let Err(e) = cr.fill() {
						eprintln!("failed to fill background: {}", e);
					}
				}
			} else if let Some(hovered_idx) = state.hovered_window
				&& let Some(win) = state.windows.get(hovered_idx)
			{
				let offset_x = surface_data.output.logical_position.x as f64;
				let offset_y = surface_data.output.logical_position.y as f64;
				let win_x = win.x as f64 - offset_x;
				let win_y = win.y as f64 - offset_y;
				let win_w = win.width as f64;
				let win_h = win.height as f64;

				cr.rectangle(win_x, win_y, win_w, win_h);
				cr.set_fill_rule(cairo::FillRule::EvenOdd);
				if let Err(e) = cr.fill() {
					eprintln!("failed to fill hovered window: {}", e);
				}
				cr.set_fill_rule(cairo::FillRule::Winding);

				graphics::set_source_color(&cr, state.config.border_color);
				cr.set_line_width(state.config.border_width);
				cr.rectangle(win_x, win_y, win_w, win_h);
				if let Err(e) = cr.stroke() {
					eprintln!("failed to stroke hovered window: {}", e);
				}

				let dim_text = format!("{}x{}", win_w as u32, win_h as u32);
				let layout = create_layout(&cr);
				layout.set_text(&dim_text);
				cr.set_source_rgb(1.0, 1.0, 1.0);
				cr.move_to(win_x, win_y - 20.0);
				show_layout(&cr, &layout);
			} else {
				if let Err(e) = cr.fill() {
					eprintln!("failed to fill background: {}", e);
				}
			}

			Self::draw_toolbar(
				&cr,
				width as f64,
				state.active_tool,
				state.current,
				surface_data.output.logical_position,
				&state.config,
			);
		}

		cairo_surface.flush();
		let cairo_data = cairo_surface
			.data()
			.map_err(|e| anyhow!("failed to get surface data: {}", e))?;
		canvas.copy_from_slice(&cairo_data);

		surface_data
			.wl_surface
			.attach(Some(buffer.wl_buffer()), 0, 0);
		surface_data
			.wl_surface
			.damage(0, 0, width as i32, height as i32);
		surface_data.wl_surface.commit();

		surface_data.slot = Some(buffer);
		Ok(())
	}

	fn draw_toolbar(
		cr: &Context,
		width: f64,
		active: Tool,
		mouse_global: (f64, f64),
		offset: Position,
		config: &SelectionConfig,
	) {
		let tools = Tool::all();

		let item_w = config.toolbar_item_width;
		let h = config.toolbar_height;
		let total_w = item_w * tools.len() as f64;
		let x = (width - total_w) / 2.0;
		let y = config.toolbar_y;

		let mouse_x = mouse_global.0 - offset.x as f64;
		let mouse_y = mouse_global.1 - offset.y as f64;

		graphics::set_source_color(cr, config.toolbar_background_color);
		cr.rectangle(x, y, total_w, h);
		cr.fill().ok();

		let mut hovered_tooltip = None;

		for (i, (tool, icon, tip)) in tools.iter().enumerate() {
			let tx = x + i as f64 * item_w;

			let is_hovered =
				mouse_x >= tx && mouse_x <= tx + item_w && mouse_y >= y && mouse_y <= y + h;

			if *tool == active {
				graphics::set_source_color(cr, config.toolbar_active_color);
				cr.rectangle(tx, y, item_w, h);
				cr.fill().ok();
			} else if is_hovered {
				graphics::set_source_color(cr, config.toolbar_hover_color);
				cr.rectangle(tx, y, item_w, h);
				cr.fill().ok();
			}

			if is_hovered {
				hovered_tooltip = Some((*tip, tx + (item_w / 2.0), y + h + 10.0));
			}

			cr.set_source_rgb(1.0, 1.0, 1.0);
			let layout = create_layout(cr);
			layout.set_text(icon);
			let font = pango::FontDescription::from_string("system-ui 18");
			layout.set_font_description(Some(&font));

			let (_, logical_rect) = layout.pixel_extents();
			let icon_w = logical_rect.width() as f64;
			let icon_h = logical_rect.height() as f64;

			cr.move_to(tx + (item_w - icon_w) / 2.0, y + (h - icon_h) / 2.0);
			show_layout(cr, &layout);
		}

		if let Some((tip, t_x, t_y)) = hovered_tooltip {
			let layout = create_layout(cr);
			layout.set_text(tip);
			let font = pango::FontDescription::from_string("system-ui Bold 12");
			layout.set_font_description(Some(&font));

			let (_, logical_rect) = layout.pixel_extents();
			let text_w = logical_rect.width() as f64;
			let text_h = logical_rect.height() as f64;

			let padding_x = 16.0;
			let padding_y = 10.0;
			let tip_w = text_w + padding_x;
			let tip_h = text_h + padding_y;

			let adjusted_x = t_x - (tip_w / 2.0);

			cr.set_source_rgba(0.0, 0.0, 0.0, 0.9);
			cr.rectangle(adjusted_x, t_y, tip_w, tip_h);
			cr.fill().ok();

			cr.set_source_rgb(1.0, 1.0, 1.0);
			cr.move_to(
				adjusted_x + (tip_w - text_w) / 2.0,
				t_y + (tip_h - text_h) / 2.0,
			);
			show_layout(cr, &layout);
		}
	}
}

impl ProvidesRegistryState for AppState {
	fn registry(&mut self) -> &mut RegistryState {
		&mut self.registry_state
	}
	smithay_client_toolkit::registry_handlers!(AppState);
}

delegate_registry!(AppState);
delegate_compositor!(AppState);
delegate_shm!(AppState);
delegate_output!(AppState);
delegate_layer!(AppState);
delegate_seat!(AppState);
delegate_pointer!(AppState);
delegate_keyboard!(AppState);

#[rustfmt::skip]
impl RegistryHandler<AppState> for AppState {
    fn new_global(_: &mut AppState,_: &Connection,_: &QueueHandle<Self>,_: u32,_: &str,_: u32,) {}
    fn remove_global(_: &mut AppState, _: &Connection, _: &QueueHandle<Self>, _: u32, _: &str) {}
}

impl OutputHandler for AppState {
	fn output_state(&mut self) -> &mut OutputState {
		&mut self.output_state
	}
	fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
	fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
	fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for AppState {
	fn shm_state(&mut self) -> &mut Shm {
		&mut self.shm_state
	}
}
#[rustfmt::skip]
impl LayerShellHandler for AppState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(
        &mut self,_: &Connection,_: &QueueHandle<Self>,
        layer: &LayerSurface,
        _: smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure,_: u32,
    ) {
        layer.wl_surface().commit();
        self.state.lock().unwrap().dirty = true;
    }
}
#[rustfmt::skip]
impl SeatHandler for AppState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,_: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            self.seat_state
                .get_pointer(qh, &seat)
                .expect("failed to get pointer");
        }
        if capability == Capability::Keyboard {
            self.seat_state
                .get_keyboard(qh, &seat, None)
                .expect("failed to get keyboard");
        }
    }
    fn remove_capability(
        &mut self,_: &Connection,_: &QueueHandle<Self>,_: wl_seat::WlSeat,_: Capability,
    ) {
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

#[rustfmt::skip]
impl PointerHandler for AppState {
    fn pointer_frame(
        &mut self,_: &Connection,qh: &QueueHandle<Self>,pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let mut state = self.state.lock().unwrap();
        for event in events {
            if let PointerEventKind::Enter { serial, .. } = event.kind {
                if self.cursor_shape_device.is_none()
                    && let Some(ref mgr) = self.cursor_shape_manager {
                        self.cursor_shape_device = Some(mgr.get_shape_device(pointer, qh));
                    }
                if let Some(ref device) = self.cursor_shape_device {
                    device.set_shape(serial, Shape::Crosshair);
                }
            }

            if let PointerEventKind::Enter { .. } = event.kind &&
                let Some(sd) = self.surfaces.iter().find(|s| s.wl_surface == event.surface) {
                state.handle_pointer_enter(sd.dimensions.0 as f64, (
                    sd.output.logical_position.x as f64,
                    sd.output.logical_position.y as f64,
                ));
            }

            let global_pos = (
                event.position.0 + state.current_offset.0,
                event.position.1 + state.current_offset.1,
            );

            match event.kind {
                PointerEventKind::Press { button, .. } => {
                    state.handle_pointer_press(global_pos, event.position, button, self.modifiers.ctrl);
                }
                PointerEventKind::Release { button, .. } => {
                    state.handle_pointer_release(global_pos, button);
                }
                PointerEventKind::Motion { .. } => {
                    state.handle_pointer_motion(global_pos, self.modifiers.shift);
                }
                _ => {}
            }
        }
    }
}
#[rustfmt::skip]
impl KeyboardHandler for AppState {
    fn press_key(
        &mut self,_: &Connection,_: &QueueHandle<Self>,_: &WlKeyboard,_: u32,
        event: KeyEvent,
    ) {
        let mut s = self.state.lock().unwrap();

        if let Some(idx) = s.editing_text_idx {
            match event.keysym {
                Keysym::Return
                | Keysym::Escape => {
                    s.editing_text_idx = None;
                }
                Keysym::BackSpace => {
                    if let Some(ref mut text) = s.annotations[idx].text {
                        text.pop();
                    }
                }
                _ => {
                    if let Some(ref txt) = event.utf8
                        && txt.chars().all(|c| !c.is_control())
                            && let Some(ref mut text) = s.annotations[idx].text {
                                text.push_str(txt);
                            }
                }
            }
            s.dirty = true;
            return;
        }

        match event.keysym {
            Keysym::Return => s.finished = true,
            Keysym::Escape => s.cancelled = true,
            Keysym::BackSpace => {
                if !s.annotations.is_empty() {
                    s.push_undo();
                    s.annotations.pop();
                    s.dirty = true;
                }
            }
            Keysym::Delete => {
                if let Some(idx) = s.selected_annotation {
                    s.push_undo();
                    s.annotations.remove(idx);
                    s.selected_annotation = None;
                    s.dirty = true;
                }
            }
            Keysym::z | Keysym::Z => {
                if self.modifiers.ctrl {
                    if self.modifiers.shift {
                        s.redo();
                    } else {
                        s.undo();
                    }
                }
            }
            Keysym::y | Keysym::Y => {
                if self.modifiers.ctrl {
                    s.redo();
                }
            }
            Keysym::d | Keysym::D if self.modifiers.ctrl => {
                s.duplicate_selected();
            }
            Keysym::bracketleft => {
                if self.modifiers.ctrl {
                    if self.modifiers.shift {
                        s.move_selected_to_back();
                    } else {
                        s.move_selected_down();
                    }
                }
            }
            Keysym::bracketright => {
                if self.modifiers.ctrl {
                    if self.modifiers.shift {
                        s.move_selected_to_front();
                    } else {
                        s.move_selected_up();
                    }
                }
            }
            _ => {
                for (tool, _, _) in Tool::all() {
                    if tool.keysyms().contains(&event.keysym) {
                        s.active_tool = *tool;
                        s.selected_annotation = None;
                        s.dirty = true;
                        return;
                    }
                }
            }
        }
    }
    fn update_modifiers(&mut self,	_: &Connection,	_: &QueueHandle<Self>,	_: &WlKeyboard,	_: u32,	modifiers: Modifiers,	_: RawModifiers,	_: u32,) {
        self.modifiers = modifiers;
    }
    fn release_key(	&mut self,	_: &Connection,	_: &QueueHandle<Self>,	_: &WlKeyboard,	_: u32,	_: KeyEvent,) {}
    fn repeat_key(&mut self,_: &Connection,	_: &QueueHandle<Self>,	_: &wl_keyboard::WlKeyboard,	_: u32,	_: KeyEvent) {}
    fn enter(&mut self,	_: &Connection,	_: &QueueHandle<Self>,	_: &WlKeyboard,	_: &WlSurface,	_: u32,	_: &[u32],	_: &[Keysym]){}
    fn leave(&mut self,	_: &Connection,	_: &QueueHandle<Self>,	_: &WlKeyboard,	_: &WlSurface,	_: u32) {}
}

#[rustfmt::skip]
impl smithay_client_toolkit::compositor::CompositorHandler for AppState {
    fn scale_factor_changed(&mut self,_: &Connection,_: &QueueHandle<Self>,_: &wl_surface::WlSurface,_: i32,) {}
    fn surface_enter(&mut self,_: &Connection,_: &QueueHandle<Self>,_: &wl_surface::WlSurface,_: &wl_output::WlOutput,) {}
    fn surface_leave(&mut self,_: &Connection,_: &QueueHandle<Self>,_: &wl_surface::WlSurface,_: &wl_output::WlOutput,) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn transform_changed(&mut self,_: &Connection,_: &QueueHandle<Self>,_: &WlSurface,_: wayland_client::protocol::wl_output::Transform) {}
}
