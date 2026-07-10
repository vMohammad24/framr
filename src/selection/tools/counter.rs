use cairo::{Antialias, Context, FontSlant, FontWeight};
use libframr::OutputInfo;
use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::config::SelectionConfig;
use crate::selection::graphics;
use crate::selection::state::{Annotation, SelectionState, Tool};
use crate::selection::tools::{MouseButton, ToolBehavior};

use super::helpers::{begin_annotation, try_pick_annotation};

const RADIUS: f64 = 14.0;

pub struct CounterTool;

impl ToolBehavior for CounterTool {
	fn icon(&self) -> &'static str {
		"󰲠"
	}

	fn tooltip(&self) -> &'static str {
		"Counter"
	}

	fn keys(&self) -> Vec<Keysym> {
		vec![Keysym::_0, Keysym::n, Keysym::N]
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
		let count = state
			.annotations
			.iter()
			.filter(|a| a.tool == Tool::Counter)
			.count();
		if let Some(ann) = state.annotations.last_mut() {
			ann.text = Some(count.to_string());
		}
	}

	fn draw(&self, cr: &Context, ann: &Annotation, output: &OutputInfo, _config: &SelectionConfig) {
		if ann.points.is_empty() {
			return;
		}

		let offset_x = output.logical_position.x as f64;
		let offset_y = output.logical_position.y as f64;
		let (x, y) = (ann.points[0].0 - offset_x, ann.points[0].1 - offset_y);

		graphics::set_source_color(cr, ann.color);
		cr.set_antialias(Antialias::Best);
		cr.arc(x, y, RADIUS, 0.0, 2.0 * std::f64::consts::PI);
		if let Err(e) = cr.fill() {
			eprintln!("failed to fill counter: {}", e);
		}

		let number = ann.text.as_deref().unwrap_or("?");
		cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
		cr.select_font_face("sans-serif", FontSlant::Normal, FontWeight::Bold);
		cr.set_font_size(16.0);
		if let Ok(ext) = cr.text_extents(number) {
			cr.move_to(
				x - ext.width() / 2.0 - ext.x_bearing(),
				y - ext.height() / 2.0 - ext.y_bearing(),
			);
			if let Err(e) = cr.show_text(number) {
				eprintln!("failed to draw counter number: {}", e);
			}
		}
	}

	fn hit_test(&self, ann: &Annotation, point: (f64, f64), threshold: f64) -> bool {
		if ann.points.is_empty() {
			return false;
		}
		let dx = point.0 - ann.points[0].0;
		let dy = point.1 - ann.points[0].1;
		(dx * dx + dy * dy).sqrt() <= RADIUS + threshold
	}
}
