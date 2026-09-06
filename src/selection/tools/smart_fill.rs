mod reconstruction;

use cairo::{Context, Extend, Filter, Format, ImageSurface, SurfacePattern};
use image::{Rgba, RgbaImage};
use libframr::OutputInfo;
use smithay_client_toolkit::seat::keyboard::Keysym;

use crate::config::{Color, SelectionConfig};
use crate::selection::backend::wayland::SurfaceData;
use crate::selection::graphics;
use crate::selection::state::{Annotation, SelectionState, SmartFillPattern, Tool};
use crate::selection::tools::{MouseButton, ToolBehavior};

use super::helpers::{begin_annotation, region_hit_test, try_pick_annotation, two_point_motion};

pub struct SmartFillTool;

impl SmartFillTool {
	fn fallback_color(state: &SelectionState) -> Color {
		Color {
			r: state.config.background_color.r,
			g: state.config.background_color.g,
			b: state.config.background_color.b,
			a: u8::MAX,
		}
	}

	fn reconstruct_annotation(state: &mut SelectionState, index: usize) -> bool {
		let region = state
			.annotations
			.get(index)
			.filter(|ann| ann.tool == Tool::SmartFill)
			.and_then(Annotation::rectangular_region);
		let Some(region) = region else {
			return false;
		};
		let fallback = Self::fallback_color(state);
		let (pattern, representative) =
			reconstruction::reconstruct(region, &state.source_images, fallback);
		if let Some(annotation) = state.annotations.get_mut(index)
			&& annotation.tool == Tool::SmartFill
		{
			annotation.color = representative;
			annotation.smart_fill = Some(pattern);
		}
		true
	}

	fn pattern_color(annotation: &Annotation, x: u32, y: u32, width: u32, height: u32) -> Color {
		let Some(pattern) = annotation.smart_fill.as_ref() else {
			return annotation.color;
		};
		let u = if width <= 1 {
			0.5
		} else {
			x as f32 / (width - 1) as f32
		};
		let v = if height <= 1 {
			0.5
		} else {
			y as f32 / (height - 1) as f32
		};
		reconstruction::sample_pattern(pattern, u, v, annotation.color)
	}

	fn draw_field(
		cr: &Context,
		pixels: &[Color],
		field_width: u32,
		field_height: u32,
		destination: (f64, f64, f64, f64),
	) {
		if field_width == 0
			|| field_height == 0
			|| pixels.len() != (field_width * field_height) as usize
		{
			return;
		}

		let mut data = Vec::with_capacity(pixels.len() * 4);
		for color in pixels {
			data.extend_from_slice(&[color.b, color.g, color.r, u8::MAX]);
		}
		let Ok(surface) = ImageSurface::create_for_data(
			data,
			Format::ARgb32,
			field_width as i32,
			field_height as i32,
			(field_width * 4) as i32,
		) else {
			eprintln!("failed to create smart fill preview surface");
			return;
		};
		let pattern = SurfacePattern::create(&surface);
		pattern.set_filter(Filter::Bilinear);
		pattern.set_extend(Extend::Pad);

		if let Err(error) = cr.save() {
			eprintln!("failed to save smart fill cairo state: {error}");
			return;
		}
		let (x, y, width, height) = destination;
		cr.translate(x, y);
		cr.scale(width / field_width as f64, height / field_height as f64);
		cr.rectangle(0.0, 0.0, field_width as f64, field_height as f64);
		match cr.set_source(&pattern) {
			Ok(()) => {
				if let Err(error) = cr.fill() {
					eprintln!("failed to fill smart fill field: {error}");
				}
			}
			Err(error) => eprintln!("failed to set smart fill field source: {error}"),
		}
		if let Err(error) = cr.restore() {
			eprintln!("failed to restore smart fill cairo state: {error}");
		}
	}
}

impl ToolBehavior for SmartFillTool {
	fn icon(&self) -> &'static str {
		""
	}

	fn tooltip(&self) -> &'static str {
		"Smart Fill"
	}

	fn keys(&self) -> Vec<Keysym> {
		vec![Keysym::f, Keysym::F]
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

		let index = begin_annotation(state, global_pos);
		let fallback = Self::fallback_color(state);
		if let Some(annotation) = state.annotations.get_mut(index) {
			annotation.color = fallback;
			annotation.smart_fill = Some(SmartFillPattern::Solid(fallback));
		}
	}

	fn on_release(
		&self,
		state: &mut SelectionState,
		global_pos: (f64, f64),
		_button: MouseButton,
		_config: &SelectionConfig,
	) {
		let Some(index) = state.pending_annotation_index() else {
			return;
		};
		let Some(annotation) = state.annotations.get_mut(index) else {
			return;
		};
		if annotation.tool != Tool::SmartFill {
			return;
		}
		if annotation.points.len() > 1 {
			annotation.points[1] = global_pos;
		} else {
			annotation.points.push(global_pos);
		}
		if !Self::reconstruct_annotation(state, index) {
			state.discard_pending_annotation_history();
		}
	}

	fn on_motion(
		&self,
		state: &mut SelectionState,
		global_pos: (f64, f64),
		_shift_pressed: bool,
		_alt_pressed: bool,
	) {
		two_point_motion(state, global_pos);
	}

	fn on_resize_finished(&self, state: &mut SelectionState, annotation_index: usize) {
		Self::reconstruct_annotation(state, annotation_index);
	}

	fn on_move_finished(&self, state: &mut SelectionState, annotation_index: usize) {
		Self::reconstruct_annotation(state, annotation_index);
	}

	fn requires_full_move_history(&self) -> bool {
		true
	}

	fn is_resizable(&self) -> bool {
		true
	}

	fn hit_test(&self, ann: &Annotation, point: (f64, f64), _threshold: f64) -> bool {
		region_hit_test(ann, point)
	}

	fn apply_effect(
		&self,
		image: &mut RgbaImage,
		ann: &Annotation,
		output: &OutputInfo,
		_config: &SelectionConfig,
	) {
		let Some((left, top, right, bottom)) = ann
			.rectangular_region()
			.map(|region| region.integer_bounds())
		else {
			return;
		};
		let output_left = output.logical_position.x as i64;
		let output_top = output.logical_position.y as i64;
		let clipped_left = left.max(output_left);
		let clipped_top = top.max(output_top);
		let clipped_right = right.min(output_left + image.width() as i64);
		let clipped_bottom = bottom.min(output_top + image.height() as i64);
		if clipped_left >= clipped_right || clipped_top >= clipped_bottom {
			return;
		}
		let width = (right - left) as u32;
		let height = (bottom - top) as u32;

		for global_y in clipped_top..clipped_bottom {
			for global_x in clipped_left..clipped_right {
				let color = Self::pattern_color(
					ann,
					(global_x - left) as u32,
					(global_y - top) as u32,
					width,
					height,
				);
				let pixel_x = (global_x - output_left) as u32;
				let pixel_y = (global_y - output_top) as u32;
				image.put_pixel(pixel_x, pixel_y, Rgba([color.r, color.g, color.b, u8::MAX]));
			}
		}
	}

	fn is_region_effect(&self) -> bool {
		true
	}

	fn render(
		&self,
		cr: &Context,
		ann: &Annotation,
		surface_data: &SurfaceData,
		_config: &SelectionConfig,
	) {
		let Some((left, top, right, bottom)) = ann
			.rectangular_region()
			.map(|region| region.integer_bounds())
		else {
			return;
		};
		let offset_x = surface_data.output.logical_position.x as f64;
		let offset_y = surface_data.output.logical_position.y as f64;
		let x = left as f64 - offset_x;
		let y = top as f64 - offset_y;
		let width = (right - left) as f64;
		let height = (bottom - top) as f64;

		match ann.smart_fill.as_ref() {
			Some(SmartFillPattern::Field {
				width: field_width,
				height: field_height,
				pixels,
			}) => Self::draw_field(
				cr,
				pixels,
				*field_width,
				*field_height,
				(x, y, width, height),
			),
			Some(SmartFillPattern::Solid(color)) => {
				graphics::set_source_color(cr, *color);
				cr.rectangle(x, y, width, height);
				if let Err(error) = cr.fill() {
					eprintln!("failed to fill smart fill annotation: {error}");
				}
			}
			None => {}
		}
	}
}
