use crate::config::{Color, SelectionConfig};
use crate::selection::tools::*;
use crate::selection::window::Window;
use smithay_client_toolkit::seat::keyboard::Keysym;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tool {
	Select,
	Circle,
	Rectangle,
	Arrow,
	Checkmark,
	Counter,
	Blur,
	Pixelate,
	Highlight,
	Text,
	Annotate,
}

impl Tool {
	pub fn behavior(&self) -> &'static dyn ToolBehavior {
		match self {
			Tool::Select => &SelectTool,
			Tool::Circle => &CircleTool,
			Tool::Rectangle => &RectangleTool,
			Tool::Arrow => &ArrowTool,
			Tool::Checkmark => &CheckmarkTool,
			Tool::Counter => &CounterTool,
			Tool::Blur => &BlurTool,
			Tool::Pixelate => &PixelateTool,
			Tool::Highlight => &HighlightTool,
			Tool::Text => &TextTool,
			Tool::Annotate => &AnnotateTool,
		}
	}

	pub fn all() -> &'static [Tool] {
		&[
			Tool::Select,
			Tool::Circle,
			Tool::Rectangle,
			Tool::Arrow,
			Tool::Checkmark,
			Tool::Counter,
			Tool::Blur,
			Tool::Pixelate,
			Tool::Highlight,
			Tool::Text,
			Tool::Annotate,
		]
	}

	pub fn from_index(index: usize) -> Self {
		Tool::all().get(index).copied().unwrap_or(Tool::Select)
	}

	pub fn from_keysym(keysym: Keysym) -> Option<Tool> {
		Tool::all()
			.iter()
			.find(|t| t.behavior().keys().contains(&keysym))
			.copied()
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
	pub windows: Vec<Window>,
	pub hovered_window: Option<usize>,
}

impl SelectionState {
	pub fn handle_pointer_enter(&mut self, surface_width: f64, offset: (f64, f64)) {
		self.last_surface_width = surface_width;
		self.current_offset = offset;
	}

	pub fn handle_pointer_press(
		&mut self,
		global_pos: (f64, f64),
		local_pos: (f64, f64),
		button: u32,
		ctrl_pressed: bool,
	) {
		self.current = global_pos;
		let mouse_btn = MouseButton::from_raw(button);

		if mouse_btn == MouseButton::Left {
			let ty = self.config.toolbar_y;
			let th = self.config.toolbar_height;
			if local_pos.1 >= ty && local_pos.1 <= ty + th {
				let item_w = self.config.toolbar_item_width;
				let total_w = item_w * Tool::all().len() as f64;
				let x_start = (self.last_surface_width - total_w) / 2.0;

				for i in 0..Tool::all().len() {
					let tx = x_start + i as f64 * item_w;
					if local_pos.0 >= tx && local_pos.0 <= tx + item_w {
						self.active_tool = Tool::from_index(i);
						self.selected_annotation = None;
						self.dirty = true;
						return;
					}
				}
			}
		}

		if mouse_btn == MouseButton::Left {
			let config = self.config;
			let behavior = self.active_tool.behavior();
			behavior.on_press(
				self,
				global_pos,
				local_pos,
				mouse_btn,
				ctrl_pressed,
				&config,
			);
		}

		if mouse_btn == MouseButton::Right {
			if self.is_dragging {
				self.is_dragging = false;
				if self.active_tool == Tool::Select {
					self.start = None;
					self.end = None;
				} else {
					self.annotations.pop();
				}
			} else {
				self.cancelled = true;
			}
		}
		self.dirty = true;
	}

	pub fn handle_pointer_release(&mut self, global_pos: (f64, f64), button: u32) {
		self.current = global_pos;
		let mouse_btn = MouseButton::from_raw(button);
		if mouse_btn == MouseButton::Left {
			if self.is_moving_annotation {
				self.is_moving_annotation = false;
				self.move_start_point = None;
				self.original_points = None;
			}
			if self.is_dragging {
				self.is_dragging = false;
				let config = self.config;
				self.active_tool
					.behavior()
					.on_release(self, global_pos, mouse_btn, &config);
			}
		}
		self.dirty = true;
	}

	pub fn handle_pointer_motion(&mut self, global_pos: (f64, f64), shift_pressed: bool) {
		self.current = global_pos;

		if self.is_moving_annotation {
			if let (Some(start), Some(orig), Some(idx)) = (
				self.move_start_point,
				&self.original_points,
				self.selected_annotation,
			) {
				let mut dx = global_pos.0 - start.0;
				let mut dy = global_pos.1 - start.1;

				if shift_pressed {
					if dx.abs() > dy.abs() {
						dy = 0.0;
					} else {
						dx = 0.0;
					}
				}

				for (i, p) in self.annotations[idx].points.iter_mut().enumerate() {
					p.0 = orig[i].0 + dx;
					p.1 = orig[i].1 + dy;
				}
			}
		} else {
			self.active_tool
				.behavior()
				.on_motion(self, global_pos, shift_pressed);
		}
		self.dirty = true;
	}

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
