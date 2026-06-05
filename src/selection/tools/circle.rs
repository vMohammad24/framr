use cairo::{Antialias, Context, LineCap, LineJoin};
use libframr::OutputInfo;
use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::config::SelectionConfig;
use crate::selection::graphics;
use crate::selection::state::{Annotation, SelectionState};
use crate::selection::tools::{MouseButton, ToolBehavior};

use super::helpers::{begin_annotation, try_pick_annotation};

pub struct CircleTool;

impl ToolBehavior for CircleTool {
	fn icon(&self) -> &'static str {
		""
	}

	fn tooltip(&self) -> &'static str {
		"Draw Circle"
	}

	fn keys(&self) -> Vec<Keysym> {
		vec![Keysym::_2, Keysym::c, Keysym::C]
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
		if state.is_dragging
			&& let Some(ann) = state.annotations.last_mut()
		{
			if ann.points.len() > 1 {
				ann.points[1] = global_pos;
			} else {
				ann.points.push(global_pos);
			}
		}
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
		let dx = x1 - x2;
		let dy = y1 - y2;
		let radius = (dx * dx + dy * dy).sqrt();

		cr.arc(x1, y1, radius, 0.0, 2.0 * std::f64::consts::PI);
		if let Err(e) = cr.stroke() {
			eprintln!("failed to stroke circle: {}", e);
		}
	}

	fn hit_test(&self, ann: &Annotation, point: (f64, f64), threshold: f64) -> bool {
		if ann.points.len() < 2 {
			return false;
		}
		let center = ann.points[0];
		let edge = ann.points[1];
		let dx = center.0 - edge.0;
		let dy = center.1 - edge.1;
		let radius = (dx * dx + dy * dy).sqrt();

		let pdx = center.0 - point.0;
		let pdy = center.1 - point.1;
		let dist = (pdx * pdx + pdy * pdy).sqrt();

		(dist - radius).abs() <= threshold
	}
}
