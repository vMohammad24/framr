use crate::config::{Color, SelectionConfig};
use smithay_client_toolkit::seat::keyboard::Keysym;
use std::collections::VecDeque;

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

	pub fn keysyms(&self) -> Vec<Keysym> {
		match self {
			Tool::Select => vec![Keysym::_1, Keysym::s, Keysym::S],
			Tool::Circle => vec![Keysym::_2, Keysym::c, Keysym::C],
			Tool::Arrow => vec![Keysym::_3, Keysym::a, Keysym::A],
			Tool::Checkmark => vec![Keysym::_4, Keysym::k, Keysym::K],
			Tool::Blur => vec![Keysym::_5, Keysym::b, Keysym::B],
			Tool::Pixelate => vec![Keysym::_6, Keysym::p, Keysym::P],
			Tool::Text => vec![Keysym::_7, Keysym::t, Keysym::T],
			Tool::Annotate => vec![Keysym::_8, Keysym::d, Keysym::D],
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
	pub undo_stack: VecDeque<Vec<Annotation>>,
	pub redo_stack: VecDeque<Vec<Annotation>>,
	pub selected_annotation: Option<usize>,
	pub is_moving_annotation: bool,
	pub move_start_point: Option<(f64, f64)>,
	pub original_points: Option<Vec<(f64, f64)>>,
	pub finished: bool,
	pub cancelled: bool,
	pub last_surface_width: f64,
	pub dirty: bool,
	pub current_offset: (f64, f64),
	pub editing_text_idx: Option<usize>,
	pub config: SelectionConfig,
}

impl SelectionState {
	pub fn push_undo(&mut self) {
		self.undo_stack.push_back(self.annotations.clone());
		if self.undo_stack.len() > 50 {
			self.undo_stack.pop_front();
		}
		self.redo_stack.clear();
	}

	pub fn undo(&mut self) {
		if let Some(prev) = self.undo_stack.pop_back() {
			self.redo_stack.push_back(self.annotations.clone());
			if self.redo_stack.len() > 50 {
				self.redo_stack.pop_front();
			}
			self.annotations = prev;
			self.dirty = true;
		}
	}

	pub fn redo(&mut self) {
		if let Some(next) = self.redo_stack.pop_back() {
			self.undo_stack.push_back(self.annotations.clone());
			if self.undo_stack.len() > 50 {
				self.undo_stack.pop_front();
			}
			self.annotations = next;
			self.dirty = true;
		}
	}

	pub fn move_selected_up(&mut self) {
		if let Some(idx) = self.selected_annotation
			&& idx < self.annotations.len() - 1
		{
			self.push_undo();
			self.annotations.swap(idx, idx + 1);
			self.selected_annotation = Some(idx + 1);
			self.dirty = true;
		}
	}

	pub fn move_selected_down(&mut self) {
		if let Some(idx) = self.selected_annotation
			&& idx > 0
		{
			self.push_undo();
			self.annotations.swap(idx, idx - 1);
			self.selected_annotation = Some(idx - 1);
			self.dirty = true;
		}
	}

	pub fn move_selected_to_front(&mut self) {
		if let Some(idx) = self.selected_annotation
			&& idx < self.annotations.len() - 1
		{
			self.push_undo();
			let ann = self.annotations.remove(idx);
			self.annotations.push(ann);
			self.selected_annotation = Some(self.annotations.len() - 1);
			self.dirty = true;
		}
	}

	pub fn move_selected_to_back(&mut self) {
		if let Some(idx) = self.selected_annotation
			&& idx > 0
		{
			self.push_undo();
			let ann = self.annotations.remove(idx);
			self.annotations.insert(0, ann);
			self.selected_annotation = Some(0);
			self.dirty = true;
		}
	}

	pub fn duplicate_selected(&mut self) {
		if let Some(idx) = self.selected_annotation {
			self.push_undo();
			let mut ann = self.annotations[idx].clone();
			for p in &mut ann.points {
				p.0 += 10.0;
				p.1 += 10.0;
			}
			self.annotations.push(ann);
			self.selected_annotation = Some(self.annotations.len() - 1);
			self.dirty = true;
		}
	}
}
