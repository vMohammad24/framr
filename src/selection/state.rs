use crate::config::{Color, SelectionConfig};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tool {
	Select,
	Circle,
	Arrow,
	Checkmark,
	Blur,
	Pixelate,
	Text,
	Annotate,
}

impl Tool {
	pub fn all() -> &'static [(Tool, &'static str, &'static str)] {
		&[
			(Tool::Select, "󰒅", "Select Area"),
			(Tool::Circle, "", "Draw Circle"),
			(Tool::Arrow, "󰁜", "Draw Arrow"),
			(Tool::Checkmark, "", "Checkmark"),
			(Tool::Blur, "󰂵", "Blur Area"),
			(Tool::Pixelate, "󰋁", "Pixelate Area"),
			(Tool::Text, "󰊄", "Add Text"),
			(Tool::Annotate, "󰏫", "Free Draw"),
		]
	}

	pub fn from_index(index: usize) -> Self {
		match index {
			0 => Tool::Select,
			1 => Tool::Circle,
			2 => Tool::Arrow,
			3 => Tool::Checkmark,
			4 => Tool::Blur,
			5 => Tool::Pixelate,
			6 => Tool::Text,
			7 => Tool::Annotate,
			_ => Tool::Select,
		}
	}
}

#[derive(Clone)]
pub struct Annotation {
	pub tool: Tool,
	pub points: Vec<(f64, f64)>,
	pub text: Option<String>,
	pub color: Color,
}

pub struct SelectionState {
	pub start: Option<(f64, f64)>,
	pub end: Option<(f64, f64)>,
	pub current: (f64, f64),
	pub is_dragging: bool,
	pub active_tool: Tool,
	pub annotations: Vec<Annotation>,
	pub finished: bool,
	pub cancelled: bool,
	pub last_surface_width: f64,
	pub dirty: bool,
	pub current_offset: (f64, f64),
	pub editing_text_idx: Option<usize>,
	pub config: SelectionConfig,
}
