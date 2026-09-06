use crate::config::{Color, SelectionConfig};
use crate::selection::tools::*;
use crate::selection::window::Window;
use image::RgbaImage;
use libframr::OutputInfo;
use smithay_client_toolkit::seat::keyboard::Keysym;
use std::collections::VecDeque;
use std::sync::Arc;

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
	SmartFill,
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
			Tool::SmartFill => &SmartFillTool,
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
			Tool::SmartFill,
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

#[derive(Clone, PartialEq)]
pub struct Annotation {
	pub tool: Tool,
	pub points: Vec<(f64, f64)>,
	pub text: Option<String>,
	pub color: Color,
	pub smart_fill: Option<SmartFillPattern>,
}

impl Annotation {
	pub fn rectangular_region(&self) -> Option<SelectionRegion> {
		if self.points.len() < 2 {
			return None;
		}

		let region = SelectionRegion::new(self.points[0], self.points[1]);
		region.is_valid().then_some(region)
	}
}

#[derive(Clone, PartialEq)]
pub enum SmartFillPattern {
	Solid(Color),
	Field {
		width: u32,
		height: u32,
		pixels: Vec<Color>,
	},
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionRegion {
	pub start: (f64, f64),
	pub end: (f64, f64),
}

impl SelectionRegion {
	pub fn new(start: (f64, f64), end: (f64, f64)) -> Self {
		Self {
			start: (start.0.min(end.0), start.1.min(end.1)),
			end: (start.0.max(end.0), start.1.max(end.1)),
		}
	}

	pub fn width(self) -> f64 {
		self.end.0 - self.start.0
	}

	pub fn height(self) -> f64 {
		self.end.1 - self.start.1
	}

	pub fn is_valid(self) -> bool {
		self.width() > 0.0 && self.height() > 0.0
	}

	pub fn integer_bounds(self) -> (i64, i64, i64, i64) {
		let left = self.start.0.floor() as i64;
		let top = self.start.1.floor() as i64;
		let right = (self.end.0.ceil() as i64).max(left + 1);
		let bottom = (self.end.1.ceil() as i64).max(top + 1);
		(left, top, right, bottom)
	}

	pub fn contains(self, point: (f64, f64)) -> bool {
		point.0 >= self.start.0
			&& point.0 <= self.end.0
			&& point.1 >= self.start.1
			&& point.1 <= self.end.1
	}

	pub fn translated(self, dx: f64, dy: f64) -> Self {
		Self {
			start: (self.start.0 + dx, self.start.1 + dy),
			end: (self.end.0 + dx, self.end.1 + dy),
		}
	}

	pub fn handle_positions(self) -> [(ResizeHandle, (f64, f64)); 8] {
		let mid_x = (self.start.0 + self.end.0) / 2.0;
		let mid_y = (self.start.1 + self.end.1) / 2.0;
		[
			(ResizeHandle::NorthWest, self.start),
			(ResizeHandle::North, (mid_x, self.start.1)),
			(ResizeHandle::NorthEast, (self.end.0, self.start.1)),
			(ResizeHandle::East, (self.end.0, mid_y)),
			(ResizeHandle::SouthEast, self.end),
			(ResizeHandle::South, (mid_x, self.end.1)),
			(ResizeHandle::SouthWest, (self.start.0, self.end.1)),
			(ResizeHandle::West, (self.start.0, mid_y)),
		]
	}

	pub fn handle_at(self, point: (f64, f64), radius: f64) -> Option<ResizeHandle> {
		self.handle_positions()
			.into_iter()
			.find(|(_, handle)| {
				(point.0 - handle.0).abs() <= radius && (point.1 - handle.1).abs() <= radius
			})
			.map(|(handle, _)| handle)
	}

	pub fn resized(self, handle: ResizeHandle, point: (f64, f64)) -> Self {
		let (mut left, mut top) = self.start;
		let (mut right, mut bottom) = self.end;

		match handle {
			ResizeHandle::NorthWest => {
				left = point.0;
				top = point.1;
			}
			ResizeHandle::North => top = point.1,
			ResizeHandle::NorthEast => {
				right = point.0;
				top = point.1;
			}
			ResizeHandle::East => right = point.0,
			ResizeHandle::SouthEast => {
				right = point.0;
				bottom = point.1;
			}
			ResizeHandle::South => bottom = point.1,
			ResizeHandle::SouthWest => {
				left = point.0;
				bottom = point.1;
			}
			ResizeHandle::West => left = point.0,
		}

		Self::new((left, top), (right, bottom))
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeHandle {
	NorthWest,
	North,
	NorthEast,
	East,
	SouthEast,
	South,
	SouthWest,
	West,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionInteraction {
	Move,
	Resize(ResizeHandle),
}

pub enum HistoryAction {
	RemoveAnnotation {
		index: usize,
	},
	InsertAnnotation {
		index: usize,
		annotation: Annotation,
	},
	TranslateAnnotation {
		index: usize,
		dx: f64,
		dy: f64,
	},
	ReplaceAnnotation {
		index: usize,
		annotation: Annotation,
	},
	SwapAnnotations {
		first: usize,
		second: usize,
	},
	MoveAnnotation {
		from: usize,
		to: usize,
	},
	RemoveRegion {
		index: usize,
	},
	InsertRegion {
		index: usize,
		region: SelectionRegion,
	},
	ReplaceRegion {
		index: usize,
		region: SelectionRegion,
	},
}

pub struct SelectionState {
	pub start: Option<(f64, f64)>,
	pub end: Option<(f64, f64)>,
	pub regions: Vec<SelectionRegion>,
	pub selected_region: Option<usize>,
	pub region_interaction: Option<RegionInteraction>,
	pub original_region: Option<SelectionRegion>,
	pub current: (f64, f64),
	pub is_dragging: bool,
	pub active_tool: Tool,
	pub annotations: Vec<Annotation>,
	pub source_images: Arc<Vec<(OutputInfo, RgbaImage)>>,
	pub undo_stack: VecDeque<HistoryAction>,
	pub redo_stack: VecDeque<HistoryAction>,
	pub pending_annotation_history: Option<HistoryAction>,
	pub pending_region_history: Option<HistoryAction>,
	pub selected_annotation: Option<usize>,
	pub is_moving_annotation: bool,
	pub move_start_point: Option<(f64, f64)>,
	pub annotation_draw_origin: Option<(f64, f64)>,
	pub annotation_resize_index: Option<usize>,
	pub annotation_resize_handle: Option<ResizeHandle>,
	pub original_annotation: Option<Annotation>,
	pub annotation_move_delta: (f64, f64),
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
						self.selected_region = None;
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
			if self.annotation_resize_handle.is_some() {
				self.cancel_annotation_resize();
			} else if self.is_dragging {
				self.is_dragging = false;
				self.annotation_draw_origin = None;
				if self.active_tool == Tool::Select {
					if let (Some(idx), Some(original)) =
						(self.selected_region, self.original_region)
					{
						self.regions[idx] = original;
					}
					self.start = None;
					self.end = None;
					self.region_interaction = None;
					self.original_region = None;
					self.discard_pending_region_history();
				} else {
					self.discard_pending_annotation_history();
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
			if self.annotation_resize_handle.is_some() {
				self.finish_annotation_resize();
			} else if self.is_moving_annotation {
				self.finish_annotation_move();
				self.is_moving_annotation = false;
				self.move_start_point = None;
			}
			if self.is_dragging {
				self.is_dragging = false;
				let config = self.config;
				self.active_tool
					.behavior()
					.on_release(self, global_pos, mouse_btn, &config);
				self.commit_annotation_history();
			}
		}
		self.dirty = true;
	}

	pub fn handle_pointer_motion(
		&mut self,
		global_pos: (f64, f64),
		shift_pressed: bool,
		alt_pressed: bool,
	) {
		self.current = global_pos;

		if let (Some(handle), Some(original), Some(index)) = (
			self.annotation_resize_handle,
			self.original_annotation.as_ref(),
			self.annotation_resize_index,
		) {
			if let Some(region) = original.rectangular_region()
				&& let Some(annotation) = self.annotations.get_mut(index)
			{
				let resized = region.resized(handle, global_pos);
				annotation.points[0] = resized.start;
				annotation.points[1] = resized.end;
			}
		} else if self.is_moving_annotation {
			if let (Some(start), Some(idx)) = (self.move_start_point, self.selected_annotation) {
				let mut dx = global_pos.0 - start.0;
				let mut dy = global_pos.1 - start.1;

				if shift_pressed {
					if dx.abs() > dy.abs() {
						dy = 0.0;
					} else {
						dx = 0.0;
					}
				}

				let step_x = dx - self.annotation_move_delta.0;
				let step_y = dy - self.annotation_move_delta.1;
				for point in &mut self.annotations[idx].points {
					point.0 += step_x;
					point.1 += step_y;
				}
				self.annotation_move_delta = (dx, dy);
			}
		} else {
			self.active_tool
				.behavior()
				.on_motion(self, global_pos, shift_pressed, alt_pressed);
		}
		self.dirty = true;
	}

	fn push_history(&mut self, action: HistoryAction) {
		Self::push_bounded(&mut self.undo_stack, action);
		self.redo_stack.clear();
	}

	pub fn undo(&mut self) {
		if let Some(action) = self.undo_stack.pop_back()
			&& let Some(inverse) = self.apply_history(action)
		{
			Self::push_bounded(&mut self.redo_stack, inverse);
			self.reset_interaction_state();
		}
	}

	pub fn redo(&mut self) {
		if let Some(action) = self.redo_stack.pop_back()
			&& let Some(inverse) = self.apply_history(action)
		{
			Self::push_bounded(&mut self.undo_stack, inverse);
			self.reset_interaction_state();
		}
	}

	pub fn begin_annotation_move(&mut self, index: usize) {
		self.annotation_move_delta = (0.0, 0.0);
		self.original_annotation = self
			.annotations
			.get(index)
			.filter(|annotation| annotation.tool.behavior().requires_full_move_history())
			.cloned();
	}

	pub fn try_begin_selected_annotation_resize(
		&mut self,
		point: (f64, f64),
		handle_radius: f64,
	) -> bool {
		let Some(index) = self.selected_annotation else {
			return false;
		};
		let Some(annotation) = self.annotations.get(index) else {
			return false;
		};
		if !annotation.tool.behavior().is_resizable() {
			return false;
		}
		let Some(handle) = annotation
			.rectangular_region()
			.and_then(|region| region.handle_at(point, handle_radius))
		else {
			return false;
		};

		self.original_annotation = Some(annotation.clone());
		self.annotation_resize_index = Some(index);
		self.annotation_resize_handle = Some(handle);
		true
	}

	fn finish_annotation_resize(&mut self) {
		self.annotation_resize_handle = None;
		let index = self.annotation_resize_index.take();
		let original = self.original_annotation.take();
		if let Some(index) = index
			&& let Some(tool) = self
				.annotations
				.get(index)
				.map(|annotation| annotation.tool)
		{
			tool.behavior().on_resize_finished(self, index);
		}
		if let (Some(index), Some(original)) = (index, original)
			&& self
				.annotations
				.get(index)
				.is_some_and(|annotation| annotation != &original)
		{
			self.push_history(HistoryAction::ReplaceAnnotation {
				index,
				annotation: original,
			});
		}
	}

	fn cancel_annotation_resize(&mut self) {
		self.annotation_resize_handle = None;
		if let (Some(index), Some(original)) = (
			self.annotation_resize_index.take(),
			self.original_annotation.take(),
		) && let Some(annotation) = self.annotations.get_mut(index)
		{
			*annotation = original;
		}
	}

	fn finish_annotation_move(&mut self) {
		let delta = self.selected_annotation.map(|index| {
			(
				index,
				self.annotation_move_delta.0,
				self.annotation_move_delta.1,
			)
		});
		self.annotation_move_delta = (0.0, 0.0);

		if let Some((index, dx, dy)) = delta
			&& (dx != 0.0 || dy != 0.0)
		{
			if let Some(tool) = self
				.annotations
				.get(index)
				.map(|annotation| annotation.tool)
			{
				tool.behavior().on_move_finished(self, index);
			}

			if let Some(original) = self.original_annotation.take() {
				self.push_history(HistoryAction::ReplaceAnnotation {
					index,
					annotation: original,
				});
			} else {
				self.push_history(HistoryAction::TranslateAnnotation {
					index,
					dx: -dx,
					dy: -dy,
				});
			}
		}
		self.original_annotation = None;
	}

	pub fn add_annotation(&mut self, annotation: Annotation) -> usize {
		let index = self.annotations.len();
		self.annotations.push(annotation);
		self.push_history(HistoryAction::RemoveAnnotation { index });
		index
	}

	pub fn begin_annotation_history(&mut self, annotation: Annotation) -> usize {
		let index = self.annotations.len();
		self.annotations.push(annotation);
		self.pending_annotation_history = Some(HistoryAction::RemoveAnnotation { index });
		index
	}

	pub(crate) fn pending_annotation_index(&self) -> Option<usize> {
		match self.pending_annotation_history {
			Some(HistoryAction::RemoveAnnotation { index }) => Some(index),
			_ => None,
		}
	}

	fn commit_annotation_history(&mut self) {
		if let Some(action) = self.pending_annotation_history.take() {
			self.push_history(action);
		}
	}

	pub(crate) fn discard_pending_annotation_history(&mut self) {
		if let Some(HistoryAction::RemoveAnnotation { index }) =
			self.pending_annotation_history.take()
			&& index < self.annotations.len()
		{
			self.annotations.remove(index);
			if self.selected_annotation == Some(index) {
				self.selected_annotation = None;
			}
		}
	}

	pub fn remove_annotation(&mut self, index: usize) {
		let annotation = self.annotations.remove(index);
		self.push_history(HistoryAction::InsertAnnotation { index, annotation });
	}

	pub fn add_region(&mut self, region: SelectionRegion) -> usize {
		let index = self.regions.len();
		self.regions.push(region);
		self.push_history(HistoryAction::RemoveRegion { index });
		index
	}

	pub fn remove_region(&mut self, index: usize) {
		let region = self.regions.remove(index);
		self.push_history(HistoryAction::InsertRegion { index, region });
	}

	pub fn begin_region_history(&mut self, index: usize) {
		self.pending_region_history = Some(HistoryAction::ReplaceRegion {
			index,
			region: self.regions[index],
		});
	}

	pub fn commit_region_history(&mut self) {
		if let Some(action) = self.pending_region_history.take() {
			self.push_history(action);
		}
	}

	pub fn discard_pending_region_history(&mut self) {
		self.pending_region_history = None;
	}

	fn push_bounded(stack: &mut VecDeque<HistoryAction>, action: HistoryAction) {
		stack.push_back(action);
		if stack.len() > 50 {
			stack.pop_front();
		}
	}

	fn apply_history(&mut self, action: HistoryAction) -> Option<HistoryAction> {
		match action {
			HistoryAction::RemoveAnnotation { index } if index < self.annotations.len() => {
				let annotation = self.annotations.remove(index);
				Some(HistoryAction::InsertAnnotation { index, annotation })
			}
			HistoryAction::InsertAnnotation { index, annotation }
				if index <= self.annotations.len() =>
			{
				self.annotations.insert(index, annotation);
				Some(HistoryAction::RemoveAnnotation { index })
			}
			HistoryAction::TranslateAnnotation { index, dx, dy }
				if index < self.annotations.len() =>
			{
				for point in &mut self.annotations[index].points {
					point.0 += dx;
					point.1 += dy;
				}
				Some(HistoryAction::TranslateAnnotation {
					index,
					dx: -dx,
					dy: -dy,
				})
			}
			HistoryAction::ReplaceAnnotation { index, annotation }
				if index < self.annotations.len() =>
			{
				let annotation = std::mem::replace(&mut self.annotations[index], annotation);
				Some(HistoryAction::ReplaceAnnotation { index, annotation })
			}
			HistoryAction::SwapAnnotations { first, second }
				if first < self.annotations.len() && second < self.annotations.len() =>
			{
				self.annotations.swap(first, second);
				Some(HistoryAction::SwapAnnotations { first, second })
			}
			HistoryAction::MoveAnnotation { from, to }
				if from < self.annotations.len() && to < self.annotations.len() =>
			{
				let annotation = self.annotations.remove(from);
				self.annotations.insert(to, annotation);
				Some(HistoryAction::MoveAnnotation { from: to, to: from })
			}
			HistoryAction::RemoveRegion { index } if index < self.regions.len() => {
				let region = self.regions.remove(index);
				Some(HistoryAction::InsertRegion { index, region })
			}
			HistoryAction::InsertRegion { index, region } if index <= self.regions.len() => {
				self.regions.insert(index, region);
				Some(HistoryAction::RemoveRegion { index })
			}
			HistoryAction::ReplaceRegion { index, region } if index < self.regions.len() => {
				let region = std::mem::replace(&mut self.regions[index], region);
				Some(HistoryAction::ReplaceRegion { index, region })
			}
			_ => None,
		}
	}

	fn reset_interaction_state(&mut self) {
		self.selected_annotation = None;
		self.selected_region = None;
		self.is_moving_annotation = false;
		self.region_interaction = None;
		self.pending_annotation_history = None;
		self.pending_region_history = None;
		self.move_start_point = None;
		self.annotation_draw_origin = None;
		self.annotation_resize_index = None;
		self.annotation_resize_handle = None;
		self.original_annotation = None;
		self.annotation_move_delta = (0.0, 0.0);
		self.original_region = None;
		self.start = None;
		self.end = None;
		self.is_dragging = false;
		self.editing_text_idx = None;
		self.dirty = true;
	}

	pub fn move_selected_up(&mut self) {
		if let Some(idx) = self.selected_annotation
			&& idx < self.annotations.len() - 1
		{
			self.push_history(HistoryAction::SwapAnnotations {
				first: idx,
				second: idx + 1,
			});
			self.annotations.swap(idx, idx + 1);
			self.selected_annotation = Some(idx + 1);
			self.dirty = true;
		}
	}

	pub fn move_selected_down(&mut self) {
		if let Some(idx) = self.selected_annotation
			&& idx > 0
		{
			self.push_history(HistoryAction::SwapAnnotations {
				first: idx,
				second: idx - 1,
			});
			self.annotations.swap(idx, idx - 1);
			self.selected_annotation = Some(idx - 1);
			self.dirty = true;
		}
	}

	pub fn move_selected_to_front(&mut self) {
		if let Some(idx) = self.selected_annotation
			&& idx < self.annotations.len() - 1
		{
			let destination = self.annotations.len() - 1;
			self.push_history(HistoryAction::MoveAnnotation {
				from: destination,
				to: idx,
			});
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
			self.push_history(HistoryAction::MoveAnnotation { from: 0, to: idx });
			let ann = self.annotations.remove(idx);
			self.annotations.insert(0, ann);
			self.selected_annotation = Some(0);
			self.dirty = true;
		}
	}

	pub fn duplicate_selected(&mut self) {
		if let Some(idx) = self.selected_annotation {
			let mut ann = self.annotations[idx].clone();
			for p in &mut ann.points {
				p.0 += 10.0;
				p.1 += 10.0;
			}
			self.selected_annotation = Some(self.add_annotation(ann));
			self.dirty = true;
		}
	}

	pub fn selection_bounds(&self) -> Option<SelectionRegion> {
		let first = *self.regions.first()?;
		Some(self.regions.iter().skip(1).fold(first, |bounds, region| {
			SelectionRegion::new(
				(
					bounds.start.0.min(region.start.0),
					bounds.start.1.min(region.start.1),
				),
				(
					bounds.end.0.max(region.end.0),
					bounds.end.1.max(region.end.1),
				),
			)
		}))
	}
}
