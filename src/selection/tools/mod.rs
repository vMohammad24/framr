mod annotate;
mod arrow;
mod blur;
mod checkmark;
mod circle;
mod helpers;
mod highlight;
mod pixelate;
mod select;
mod text;

pub use annotate::AnnotateTool;
pub use arrow::ArrowTool;
pub use blur::BlurTool;
pub use checkmark::CheckmarkTool;
pub use circle::CircleTool;
pub use highlight::HighlightTool;
pub use pixelate::PixelateTool;
pub use select::SelectTool;
pub use text::TextTool;

use cairo::Context;
use image::RgbaImage;
use libframr::OutputInfo;
use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::config::SelectionConfig;
use crate::selection::backend::wayland::SurfaceData;
use crate::selection::state::{Annotation, SelectionState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
	Left,
	Right,
	Middle,
	Other(u32),
}

impl MouseButton {
	pub fn from_raw(button: u32) -> Self {
		match button {
			0x110 => MouseButton::Left,
			0x111 => MouseButton::Right,
			0x112 => MouseButton::Middle,
			other => MouseButton::Other(other),
		}
	}
}

pub trait ToolBehavior: Send + Sync {
	fn icon(&self) -> &'static str;
	fn tooltip(&self) -> &'static str;
	fn keys(&self) -> Vec<Keysym>;
	fn on_press(
		&self,
		state: &mut SelectionState,
		global_pos: (f64, f64),
		local_pos: (f64, f64),
		button: MouseButton,
		ctrl_pressed: bool,
		config: &SelectionConfig,
	);

	fn on_release(
		&self,
		_state: &mut SelectionState,
		_global_pos: (f64, f64),
		_button: MouseButton,
		_config: &SelectionConfig,
	) {
	}
	fn on_motion(
		&self,
		_state: &mut SelectionState,
		_global_pos: (f64, f64),
		_shift_pressed: bool,
	) {
	}

	fn draw(
		&self,
		_cr: &Context,
		_ann: &Annotation,
		_output: &OutputInfo,
		_config: &SelectionConfig,
	) {
	}

	fn hit_test(&self, ann: &Annotation, point: (f64, f64), threshold: f64) -> bool;

	fn apply_effect(
		&self,
		_img: &mut RgbaImage,
		_ann: &Annotation,
		_output: &OutputInfo,
		_config: &SelectionConfig,
	) {
	}

	fn is_region_effect(&self) -> bool {
		false
	}

	fn render(
		&self,
		cr: &Context,
		ann: &Annotation,
		surface_data: &SurfaceData,
		config: &SelectionConfig,
	) {
		self.draw(cr, ann, &surface_data.output, config);
	}
}
