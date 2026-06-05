use cairo::{Antialias, Context, LineCap, LineJoin};
use libframr::OutputInfo;
use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::config::SelectionConfig;
use crate::selection::graphics;
use crate::selection::state::{Annotation, SelectionState};
use crate::selection::tools::{MouseButton, ToolBehavior};

use super::helpers::{begin_annotation, try_pick_annotation, two_point_motion};

pub struct ArrowTool;

impl ToolBehavior for ArrowTool {
	fn icon(&self) -> &'static str {
		"󰁜"
	}

	fn tooltip(&self) -> &'static str {
		"Draw Arrow"
	}

	fn keys(&self) -> Vec<Keysym> {
		vec![Keysym::_3, Keysym::a, Keysym::A]
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
		if ann.points.len() < 2 {
			return;
		}

		graphics::set_source_color(cr, ann.color);
		cr.set_line_width(config.annotation_line_width);
		cr.set_antialias(Antialias::Best);
		cr.set_line_cap(LineCap::Round);
		cr.set_line_join(LineJoin::Round);

		let offset_x = output.logical_position.x as f64;
		let offset_y = output.logical_position.y as f64;
		let (x1, y1) = (ann.points[0].0 - offset_x, ann.points[0].1 - offset_y);
		let (x2, y2) = (ann.points[1].0 - offset_x, ann.points[1].1 - offset_y);

		cr.move_to(x1, y1);
		cr.line_to(x2, y2);
		if let Err(e) = cr.stroke() {
			eprintln!("failed to stroke arrow line: {}", e);
		}

		let angle = (y2 - y1).atan2(x2 - x1);
		let head_len = 20.0;
		cr.move_to(x2, y2);
		cr.line_to(
			x2 - head_len * (angle - 0.5).cos(),
			y2 - head_len * (angle - 0.5).sin(),
		);
		cr.move_to(x2, y2);
		cr.line_to(
			x2 - head_len * (angle + 0.5).cos(),
			y2 - head_len * (angle + 0.5).sin(),
		);
		if let Err(e) = cr.stroke() {
			eprintln!("failed to stroke arrow head: {}", e);
		}
	}

	fn hit_test(&self, ann: &Annotation, point: (f64, f64), threshold: f64) -> bool {
		if ann.points.len() < 2 {
			return false;
		}
		graphics::dist_to_segment(point, ann.points[0], ann.points[1]) <= threshold
	}
}
