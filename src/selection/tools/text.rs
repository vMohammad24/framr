use cairo::Context;
use libframr::OutputInfo;
use pangocairo::functions::{create_layout, show_layout};
use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::config::SelectionConfig;
use crate::selection::graphics;
use crate::selection::state::{Annotation, SelectionState};
use crate::selection::tools::{MouseButton, ToolBehavior};

use super::helpers::try_pick_annotation;

pub struct TextTool;

impl ToolBehavior for TextTool {
	fn icon(&self) -> &'static str {
		"󰊄"
	}

	fn tooltip(&self) -> &'static str {
		"Add Text"
	}

	fn keys(&self) -> Vec<Keysym> {
		vec![Keysym::_8, Keysym::t, Keysym::T]
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

		state.push_undo();
		let color = state.config.annotation_color;
		state.annotations.push(Annotation {
			tool: state.active_tool,
			points: vec![global_pos],
			text: Some(String::new()),
			color,
		});
		state.editing_text_idx = Some(state.annotations.len() - 1);
		state.selected_annotation = Some(state.annotations.len() - 1);
	}

	fn draw(&self, cr: &Context, ann: &Annotation, output: &OutputInfo, _config: &SelectionConfig) {
		let Some(text) = &ann.text else { return };
		if ann.points.is_empty() {
			return;
		}

		graphics::set_source_color(cr, ann.color);

		let offset_x = output.logical_position.x as f64;
		let offset_y = output.logical_position.y as f64;
		let (x, y) = (ann.points[0].0 - offset_x, ann.points[0].1 - offset_y);

		let layout = create_layout(cr);
		layout.set_text(text);
		let font = pango::FontDescription::from_string("system-ui Bold 20");
		layout.set_font_description(Some(&font));
		cr.move_to(x, y);
		show_layout(cr, &layout);
	}

	fn hit_test(&self, ann: &Annotation, point: (f64, f64), _threshold: f64) -> bool {
		let Some(text) = &ann.text else { return false };
		if ann.points.is_empty() {
			return false;
		}
		let p = ann.points[0];
		let (w, h) = graphics::get_text_size(text);
		let dx = point.0 - p.0;
		let dy = point.1 - p.1;
		dx >= 0.0 && dx <= w && dy >= 0.0 && dy <= h
	}
}
