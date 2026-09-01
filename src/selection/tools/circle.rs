use cairo::{Antialias, Context, LineCap, LineJoin};
use libframr::OutputInfo;
use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::config::SelectionConfig;
use crate::selection::graphics;
use crate::selection::state::{Annotation, SelectionState, Tool};
use crate::selection::tools::{MouseButton, ToolBehavior};

use super::helpers::{begin_annotation, try_pick_annotation};

pub struct CircleTool;

impl CircleTool {
	fn ellipse_geometry(ann: &Annotation) -> Option<(f64, f64, f64, f64)> {
		if ann.points.len() < 2 {
			return None;
		}

		let (x1, y1) = ann.points[0];
		let (x2, y2) = ann.points[1];

		let cx = (x1 + x2) * 0.5;
		let cy = (y1 + y2) * 0.5;
		let rx = (x2 - x1).abs() * 0.5;
		let ry = (y2 - y1).abs() * 0.5;

		if rx <= f64::EPSILON || ry <= f64::EPSILON {
			return None;
		}

		Some((cx, cy, rx, ry))
	}

	fn distance_to_ellipse(point: (f64, f64), center: (f64, f64), rx: f64, ry: f64) -> f64 {
		let px = (point.0 - center.0).abs();
		let py = (point.1 - center.1).abs();

		let distance_squared = |t: f64| {
			let dx = rx * t.cos() - px;
			let dy = ry * t.sin() - py;
			dx * dx + dy * dy
		};

		let mut left = 0.0;
		let mut right = std::f64::consts::FRAC_PI_2;
		let golden_ratio = (5.0_f64.sqrt() - 1.0) * 0.5;
		let mut inner_left = right - golden_ratio * (right - left);
		let mut inner_right = left + golden_ratio * (right - left);
		let mut left_distance = distance_squared(inner_left);
		let mut right_distance = distance_squared(inner_right);

		for _ in 0..24 {
			if left_distance < right_distance {
				right = inner_right;
				inner_right = inner_left;
				right_distance = left_distance;
				inner_left = right - golden_ratio * (right - left);
				left_distance = distance_squared(inner_left);
			} else {
				left = inner_left;
				inner_left = inner_right;
				left_distance = right_distance;
				inner_right = left + golden_ratio * (right - left);
				right_distance = distance_squared(inner_right);
			}
		}

		distance_squared((left + right) * 0.5)
			.min(distance_squared(0.0))
			.min(distance_squared(std::f64::consts::FRAC_PI_2))
			.sqrt()
	}
}

impl ToolBehavior for CircleTool {
	fn icon(&self) -> &'static str {
		""
	}

	fn tooltip(&self) -> &'static str {
		"Draw Ellipse"
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
		state.annotation_draw_origin = Some(global_pos);
	}

	fn on_release(
		&self,
		state: &mut SelectionState,
		_global_pos: (f64, f64),
		_button: MouseButton,
		_config: &SelectionConfig,
	) {
		state.annotation_draw_origin = None;

		if state
			.annotations
			.last()
			.is_some_and(|ann| ann.tool == Tool::Circle && Self::ellipse_geometry(ann).is_none())
		{
			state.discard_pending_annotation_history();
		}
	}

	fn on_motion(
		&self,
		state: &mut SelectionState,
		global_pos: (f64, f64),
		shift_pressed: bool,
		alt_pressed: bool,
	) {
		if state.is_dragging
			&& let Some(origin) = state.annotation_draw_origin
			&& let Some(ann) = state.annotations.last_mut()
			&& ann.tool == Tool::Circle
		{
			let mut dx = global_pos.0 - origin.0;
			let mut dy = global_pos.1 - origin.1;

			if shift_pressed {
				let extent = dx.abs().max(dy.abs());
				dx = extent.copysign(dx);
				dy = extent.copysign(dy);
			}

			let (start, end) = if alt_pressed {
				(
					(origin.0 - dx, origin.1 - dy),
					(origin.0 + dx, origin.1 + dy),
				)
			} else {
				(origin, (origin.0 + dx, origin.1 + dy))
			};

			if ann.points.len() == 1 {
				ann.points[0] = start;
				ann.points.push(end);
			} else {
				ann.points[0] = start;
				ann.points[1] = end;
			}
		}
	}

	fn draw(&self, cr: &Context, ann: &Annotation, output: &OutputInfo, config: &SelectionConfig) {
		let Some((cx, cy, rx, ry)) = Self::ellipse_geometry(ann) else {
			return;
		};

		graphics::set_source_color(cr, ann.color);
		cr.set_line_width(config.annotation_line_width);
		cr.set_antialias(Antialias::Best);
		cr.set_line_cap(LineCap::Round);
		cr.set_line_join(LineJoin::Round);

		let offset_x = output.logical_position.x as f64;
		let offset_y = output.logical_position.y as f64;

		if let Err(e) = cr.save() {
			eprintln!("failed to save cairo state: {e}");
			return;
		}

		cr.translate(cx - offset_x, cy - offset_y);
		cr.scale(rx, ry);

		cr.arc(0.0, 0.0, 1.0, 0.0, 2.0 * std::f64::consts::PI);

		if let Err(e) = cr.restore() {
			eprintln!("failed to restore cairo state: {e}");
			return;
		}

		if let Err(e) = cr.stroke() {
			eprintln!("failed to stroke ellipse: {e}");
		}
	}

	fn hit_test(&self, ann: &Annotation, point: (f64, f64), threshold: f64) -> bool {
		let Some((cx, cy, rx, ry)) = Self::ellipse_geometry(ann) else {
			return false;
		};

		Self::distance_to_ellipse(point, (cx, cy), rx, ry) <= threshold
	}
}
