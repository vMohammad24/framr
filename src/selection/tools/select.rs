use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::config::SelectionConfig;
use crate::selection::graphics;
use crate::selection::state::{Annotation, SelectionState};
use crate::selection::tools::{MouseButton, ToolBehavior};

pub struct SelectTool;

impl ToolBehavior for SelectTool {
	fn icon(&self) -> &'static str {
		"󰒅"
	}

	fn tooltip(&self) -> &'static str {
		"Select Area"
	}

	fn keys(&self) -> Vec<Keysym> {
		vec![Keysym::_1, Keysym::s, Keysym::S]
	}

	fn on_press(
		&self,
		state: &mut SelectionState,
		global_pos: (f64, f64),
		_local_pos: (f64, f64),
		_button: MouseButton,
		_ctrl_pressed: bool,
		_config: &SelectionConfig,
	) {
		let hovered_win = crate::selection::window::get_window_at_pos(global_pos, &state.windows);

		if hovered_win.is_some() {
			state.start = Some(global_pos);
			state.move_start_point = Some(global_pos);
			state.end = Some(global_pos);
			state.is_dragging = true;
		} else {
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
			} else {
				state.selected_annotation = None;
				state.start = Some(global_pos);
				state.move_start_point = Some(global_pos);
				state.end = None;
				state.is_dragging = true;
			}
		}
	}

	fn on_release(
		&self,
		state: &mut SelectionState,
		global_pos: (f64, f64),
		_button: MouseButton,
		_config: &SelectionConfig,
	) {
		if let Some(start) = state.start {
			let dx = (start.0 - global_pos.0).abs();
			let dy = (start.1 - global_pos.1).abs();

			if dx <= 5.0 && dy <= 5.0 {
				if let Some(hovered_idx) = state.hovered_window {
					if let Some(win) = state.windows.get(hovered_idx).cloned() {
						let win_x = win.x as f64;
						let win_y = win.y as f64;
						let win_w = win.width as f64;
						let win_h = win.height as f64;
						state.start = Some((win_x, win_y));
						state.end = Some((win_x + win_w, win_y + win_h));
					}
				} else {
					state.start = None;
					state.end = None;
				}
			} else {
				state.end = Some(global_pos);
			}
		}

		state.finished = true;
		state.move_start_point = None;
	}

	fn on_motion(&self, state: &mut SelectionState, global_pos: (f64, f64), _shift_pressed: bool) {
		if !state.is_dragging {
			state.hovered_window =
				crate::selection::window::get_window_at_pos(global_pos, &state.windows);
		}
		if state.is_dragging {
			state.end = Some(global_pos);
		}
	}

	fn hit_test(&self, _ann: &Annotation, _point: (f64, f64), _threshold: f64) -> bool {
		false
	}
}
