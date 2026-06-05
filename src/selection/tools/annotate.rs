use cairo::{Antialias, Context, LineCap, LineJoin};
use libframr::OutputInfo;
use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::config::SelectionConfig;
use crate::selection::graphics;
use crate::selection::state::{Annotation, SelectionState};
use crate::selection::tools::{MouseButton, ToolBehavior};

use super::helpers::{begin_annotation, try_pick_annotation};

pub struct AnnotateTool;

impl ToolBehavior for AnnotateTool {
	fn icon(&self) -> &'static str {
		"󰏫"
	}

	fn tooltip(&self) -> &'static str {
		"Free Draw"
	}

	fn keys(&self) -> Vec<Keysym> {
		vec![Keysym::_9, Keysym::d, Keysym::D]
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
			ann.points.push(global_pos);
		}
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
		let (x0, y0) = (ann.points[0].0 - offset_x, ann.points[0].1 - offset_y);
		cr.move_to(x0, y0);
		for p in &ann.points[1..] {
			cr.line_to(p.0 - offset_x, p.1 - offset_y);
		}
		if let Err(e) = cr.stroke() {
			eprintln!("failed to stroke annotation: {}", e);
		}
	}

	fn hit_test(&self, ann: &Annotation, point: (f64, f64), threshold: f64) -> bool {
		ann.points
			.windows(2)
			.any(|w| graphics::dist_to_segment(point, w[0], w[1]) <= threshold)
	}
}
