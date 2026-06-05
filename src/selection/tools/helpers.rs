use crate::selection::graphics;
use crate::selection::state::{Annotation, SelectionState};

pub(super) fn try_pick_annotation(state: &mut SelectionState, global_pos: (f64, f64)) -> bool {
	let hit_idx = state
		.annotations
		.iter()
		.enumerate()
		.rev()
		.find(|(_, ann)| graphics::hit_test(ann, global_pos, 5.0))
		.map(|(idx, _)| idx);

	if let Some(idx) = hit_idx {
		state.push_undo();
		state.selected_annotation = Some(idx);
		state.is_moving_annotation = true;
		state.move_start_point = Some(global_pos);
		state.original_points = Some(state.annotations[idx].points.clone());
		true
	} else {
		false
	}
}

pub(super) fn begin_annotation(state: &mut SelectionState, global_pos: (f64, f64)) {
	state.push_undo();
	let color = state.config.annotation_color;
	state.annotations.push(Annotation {
		tool: state.active_tool,
		points: vec![global_pos],
		text: None,
		color,
	});
	state.editing_text_idx = None;
	state.is_dragging = true;
}

pub(super) fn two_point_motion(state: &mut SelectionState, global_pos: (f64, f64)) {
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

pub(super) fn region_rect(
	ann: &Annotation,
	offset_x: f64,
	offset_y: f64,
) -> Option<(f64, f64, f64, f64)> {
	if ann.points.len() < 2 {
		return None;
	}
	let x1 = ann.points[0].0 - offset_x;
	let y1 = ann.points[0].1 - offset_y;
	let x2 = ann.points[1].0 - offset_x;
	let y2 = ann.points[1].1 - offset_y;
	let x = x1.min(x2);
	let y = y1.min(y2);
	let w = (x1 - x2).abs();
	let h = (y1 - y2).abs();
	if w > 0.0 && h > 0.0 {
		Some((x, y, w, h))
	} else {
		None
	}
}

pub(super) fn region_hit_test(ann: &Annotation, point: (f64, f64)) -> bool {
	if ann.points.len() < 2 {
		return false;
	}
	let (x1, y1) = ann.points[0];
	let (x2, y2) = ann.points[1];
	let x = x1.min(x2);
	let y = y1.min(y2);
	let w = (x1 - x2).abs();
	let h = (y1 - y2).abs();
	point.0 >= x && point.0 <= x + w && point.1 >= y && point.1 <= y + h
}
