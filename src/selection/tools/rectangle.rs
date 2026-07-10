use cairo::{Antialias, Context, LineJoin};
use libframr::OutputInfo;
use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::config::SelectionConfig;
use crate::selection::graphics;
use crate::selection::state::{Annotation, SelectionState};
use crate::selection::tools::{MouseButton, ToolBehavior};

use super::helpers::{begin_annotation, region_rect, try_pick_annotation, two_point_motion};

pub struct RectangleTool;

impl ToolBehavior for RectangleTool {
	fn icon(&self) -> &'static str {
		"󰹟"
	}

	fn tooltip(&self) -> &'static str {
		"Draw Rectangle"
	}

	fn keys(&self) -> Vec<Keysym> {
		vec![Keysym::r, Keysym::R]
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

	fn draw(&self, cr: &Context, ann: &Annotation, output: &OutputInfo, config: &SelectionConfig) {
		let offset_x = output.logical_position.x as f64;
		let offset_y = output.logical_position.y as f64;
		let Some((x, y, w, h)) = region_rect(ann, offset_x, offset_y) else {
			return;
		};

		graphics::set_source_color(cr, ann.color);
		cr.set_line_width(config.annotation_line_width);
		cr.set_antialias(Antialias::Best);
		cr.set_line_join(LineJoin::Round);

		cr.rectangle(x, y, w, h);
		if let Err(e) = cr.stroke() {
			eprintln!("failed to stroke rectangle: {}", e);
		}
	}

	fn hit_test(&self, ann: &Annotation, point: (f64, f64), threshold: f64) -> bool {
		if ann.points.len() < 2 {
			return false;
		}
		let (x1, y1) = ann.points[0];
		let (x2, y2) = ann.points[1];
		let x = x1.min(x2);
		let y = y1.min(y2);
		let w = (x1 - x2).abs();
		let h = (y1 - y2).abs();
		let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
		(0..4).any(|i| {
			graphics::dist_to_segment(point, corners[i], corners[(i + 1) % 4]) <= threshold
		})
	}
}
