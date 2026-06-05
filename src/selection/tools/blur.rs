use cairo::Context;
use image::RgbaImage;
use libframr::OutputInfo;
use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::config::SelectionConfig;
use crate::selection::backend::wayland::SurfaceData;
use crate::selection::graphics;
use crate::selection::state::{Annotation, SelectionState};
use crate::selection::tools::{MouseButton, ToolBehavior};

use super::helpers::{
	begin_annotation, region_hit_test, region_rect, try_pick_annotation, two_point_motion,
};

pub struct BlurTool;

impl ToolBehavior for BlurTool {
	fn icon(&self) -> &'static str {
		"󰂵"
	}

	fn tooltip(&self) -> &'static str {
		"Blur Area"
	}

	fn keys(&self) -> Vec<Keysym> {
		vec![Keysym::_5, Keysym::b, Keysym::B]
	}

	fn on_press(
		&self,
		state: &mut SelectionState,
		global_pos: (f64, f64),
		_local_pos: (f64, f64),
		_button: MouseButton,
		ctrl_pressed: bool,
		_config: &SelectionConfig,
	) {
		if ctrl_pressed {
			try_pick_annotation(state, global_pos);
			return;
		}
		begin_annotation(state, global_pos);
	}

	fn on_motion(&self, state: &mut SelectionState, global_pos: (f64, f64), _shift_pressed: bool) {
		two_point_motion(state, global_pos);
	}

	fn hit_test(&self, ann: &Annotation, point: (f64, f64), _threshold: f64) -> bool {
		region_hit_test(ann, point)
	}

	fn apply_effect(
		&self,
		img: &mut RgbaImage,
		ann: &Annotation,
		output: &OutputInfo,
		config: &SelectionConfig,
	) {
		if ann.points.len() < 2 {
			return;
		}
		let (x1, y1) = (
			ann.points[0].0 - output.logical_position.x as f64,
			ann.points[0].1 - output.logical_position.y as f64,
		);
		let (x2, y2) = (
			ann.points[1].0 - output.logical_position.x as f64,
			ann.points[1].1 - output.logical_position.y as f64,
		);
		let bx = (x1.min(x2) as u32).min(img.width());
		let by = (y1.min(y2) as u32).min(img.height());
		let bw = ((x1 - x2).abs() as u32).min(img.width() - bx);
		let bh = ((y1 - y2).abs() as u32).min(img.height() - by);
		if bw > 0 && bh > 0 {
			graphics::apply_blur(img, bx, by, bw, bh, config.blur_radius);
		}
	}

	fn is_region_effect(&self) -> bool {
		true
	}

	fn render(
		&self,
		cr: &Context,
		ann: &Annotation,
		surface_data: &SurfaceData,
		_config: &SelectionConfig,
	) {
		let offset_x = surface_data.output.logical_position.x as f64;
		let offset_y = surface_data.output.logical_position.y as f64;
		if let Some((x, y, w, h)) = region_rect(ann, offset_x, offset_y) {
			if let Err(e) = cr.set_source_surface(&surface_data.cached_blurred_bg, 0.0, 0.0) {
				eprintln!("failed to set blurred source surface: {}", e);
			}
			cr.rectangle(x, y, w, h);
			if let Err(e) = cr.fill() {
				eprintln!("failed to fill: {}", e);
			}
		}
	}
}
