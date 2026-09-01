use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::config::SelectionConfig;
use crate::selection::graphics;
use crate::selection::state::{Annotation, RegionInteraction, SelectionRegion, SelectionState};
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
		config: &SelectionConfig,
	) {
		if config.multi_region_mode
			&& let Some(idx) = state.selected_region
			&& let Some(handle) = state.regions[idx].handle_at(global_pos, 7.0)
		{
			state.begin_region_history(idx);
			state.region_interaction = Some(RegionInteraction::Resize(handle));
			state.original_region = Some(state.regions[idx]);
			state.move_start_point = Some(global_pos);
			state.is_dragging = true;
			return;
		}

		let hovered_win = crate::selection::window::get_window_at_pos(global_pos, &state.windows);
		if hovered_win.is_none() {
			let hit_idx = state
				.annotations
				.iter()
				.enumerate()
				.rev()
				.find(|(_, ann)| graphics::hit_test(ann, global_pos, 5.0))
				.map(|(idx, _)| idx);

			if let Some(idx) = hit_idx {
				state.begin_annotation_move();
				state.selected_annotation = Some(idx);
				state.is_moving_annotation = true;
				state.move_start_point = Some(global_pos);
				return;
			}
		}

		if config.multi_region_mode
			&& let Some(idx) = state
				.regions
				.iter()
				.enumerate()
				.rev()
				.find(|(_, region)| region.contains(global_pos))
				.map(|(idx, _)| idx)
		{
			state.begin_region_history(idx);
			state.selected_region = Some(idx);
			state.region_interaction = Some(RegionInteraction::Move);
			state.original_region = Some(state.regions[idx]);
			state.move_start_point = Some(global_pos);
			state.is_dragging = true;
			return;
		}

		state.selected_annotation = None;
		state.selected_region = None;
		state.region_interaction = None;
		state.original_region = None;
		state.start = Some(global_pos);
		state.move_start_point = Some(global_pos);
		state.end = Some(global_pos);
		state.is_dragging = true;
	}

	fn on_release(
		&self,
		state: &mut SelectionState,
		global_pos: (f64, f64),
		_button: MouseButton,
		config: &SelectionConfig,
	) {
		if state.region_interaction.take().is_some() {
			state.is_dragging = false;
			let changed = match (state.selected_region, state.original_region) {
				(Some(idx), Some(original)) => state.regions[idx] != original,
				_ => false,
			};
			if changed {
				state.commit_region_history();
			} else {
				state.discard_pending_region_history();
			}
			state.original_region = None;
			state.move_start_point = None;
			return;
		}

		if let Some(start) = state.start {
			let dx = (start.0 - global_pos.0).abs();
			let dy = (start.1 - global_pos.1).abs();

			if dx <= 5.0 && dy <= 5.0 {
				let hovered =
					crate::selection::window::get_window_at_pos(global_pos, &state.windows);
				if let Some(hovered_idx) = hovered {
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

		if let (Some(start), Some(end)) = (state.start, state.end) {
			let region = SelectionRegion::new(start, end);
			if region.is_valid() {
				state.selected_region = Some(state.add_region(region));
			}
		}

		state.start = None;
		state.end = None;
		if !config.multi_region_mode {
			state.finished = true;
		}
		state.move_start_point = None;
	}

	fn on_motion(
		&self,
		state: &mut SelectionState,
		global_pos: (f64, f64),
		_shift_pressed: bool,
		_alt_pressed: bool,
	) {
		if let (Some(interaction), Some(original), Some(idx), Some(drag_start)) = (
			state.region_interaction,
			state.original_region,
			state.selected_region,
			state.move_start_point,
		) {
			state.regions[idx] = match interaction {
				RegionInteraction::Move => {
					original.translated(global_pos.0 - drag_start.0, global_pos.1 - drag_start.1)
				}
				RegionInteraction::Resize(handle) => original.resized(handle, global_pos),
			};
			return;
		}

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
