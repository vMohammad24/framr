use cairo::{Antialias, Context, LineCap, LineJoin};
use libframr::OutputInfo;
use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::config::SelectionConfig;
use crate::selection::graphics;
use crate::selection::state::{Annotation, SelectionState};
use crate::selection::tools::{MouseButton, ToolBehavior};

use super::helpers::{begin_annotation, try_pick_annotation};

pub struct CheckmarkTool;

impl ToolBehavior for CheckmarkTool {
	fn icon(&self) -> &'static str {
		""
	}

	fn tooltip(&self) -> &'static str {
		"Checkmark"
	}

	fn keys(&self) -> Vec<Keysym> {
		vec![Keysym::_4, Keysym::k, Keysym::K]
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

	fn draw(&self, cr: &Context, ann: &Annotation, output: &OutputInfo, config: &SelectionConfig) {
		if ann.points.is_empty() {
			return;
		}

		graphics::set_source_color(cr, ann.color);
		cr.set_line_width(config.annotation_line_width);
		cr.set_antialias(Antialias::Best);
		cr.set_line_cap(LineCap::Round);
		cr.set_line_join(LineJoin::Round);

		let offset_x = output.logical_position.x as f64;
		let offset_y = output.logical_position.y as f64;
		let (x, y) = (ann.points[0].0 - offset_x, ann.points[0].1 - offset_y);

		cr.move_to(x - 10.0, y);
		cr.line_to(x, y + 10.0);
		cr.line_to(x + 20.0, y - 10.0);
		if let Err(e) = cr.stroke() {
			eprintln!("failed to stroke checkmark: {}", e);
		}
	}

	fn hit_test(&self, ann: &Annotation, point: (f64, f64), _threshold: f64) -> bool {
		if ann.points.is_empty() {
			return false;
		}
		let p = ann.points[0];
		let dx = (point.0 - p.0).abs();
		let dy = (point.1 - p.1).abs();
		dx <= 20.0 && dy <= 20.0
	}
}
