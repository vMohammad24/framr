use anyhow::{Result, anyhow};
use cairo::{Context, Format, ImageSurface};
use image::RgbaImage;
use libframr::OutputInfo;
use pangocairo::functions::create_layout;

use crate::config::{Color, SelectionConfig};
use crate::selection::state::Annotation;

pub fn apply_annotations(
	img: &mut RgbaImage,
	annotations: &[Annotation],
	output: &OutputInfo,
	config: &SelectionConfig,
) -> Result<()> {
	for ann in annotations {
		ann.tool.behavior().apply_effect(img, ann, output, config);
	}

	if annotations
		.iter()
		.all(|a| a.tool.behavior().is_region_effect())
	{
		return Ok(());
	}

	let (w, h) = img.dimensions();
	for pixel in img.as_mut().chunks_exact_mut(4) {
		pixel.swap(0, 2); // RGBA to BGRA
	}
	let surface = unsafe {
		ImageSurface::create_for_data_unsafe(
			img.as_mut().as_mut_ptr(),
			Format::ARgb32,
			w as i32,
			h as i32,
			(w * 4) as i32,
		)
	}
	.map_err(|e| anyhow!("failed to create cairo surface: {e}"))?;
	{
		let cr = Context::new(&surface).map_err(|e| anyhow!("failed to create context: {e}"))?;
		let scale_x = w as f64 / output.logical_size.width.max(1) as f64;
		let scale_y = h as f64 / output.logical_size.height.max(1) as f64;
		if (scale_x - 1.0).abs() > 1e-4 || (scale_y - 1.0).abs() > 1e-4 {
			cr.scale(scale_x, scale_y);
		}
		for ann in annotations {
			if !ann.tool.behavior().is_region_effect() {
				draw_annotation(&cr, ann, output, config);
			}
		}
	}

	surface.flush();
	drop(surface);

	for pixel in img.as_mut().chunks_exact_mut(4) {
		pixel.swap(0, 2); // BGRA to RGBA
	}
	Ok(())
}

pub fn apply_blur(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, radius: f32) {
	let x_end = (x + w).min(img.width());
	let y_end = (y + h).min(img.height());
	if x >= x_end || y >= y_end {
		return;
	}
	let w = x_end - x;
	let h = y_end - y;
	let mut data = vec![[0u8; 3]; (w * h) as usize];
	let stride = img.width() as usize * 4;
	let row_offset = x as usize * 4;
	let row_bytes = w as usize * 4;
	let raw = img.as_raw();

	for py in 0..h as usize {
		let src_start = (y as usize + py) * stride + row_offset;
		let src_row = &raw[src_start..src_start + row_bytes];
		let dst_row = &mut data[py * w as usize..(py + 1) * w as usize];
		for (dst, src) in dst_row.iter_mut().zip(src_row.chunks_exact(4)) {
			*dst = [src[0], src[1], src[2]];
		}
	}

	fastblur::gaussian_blur(&mut data, w as usize, h as usize, radius);

	let raw_mut = img.as_mut();
	for py in 0..h as usize {
		let dst_start = (y as usize + py) * stride + row_offset;
		let dst_row = &mut raw_mut[dst_start..dst_start + row_bytes];
		let src_row = &data[py * w as usize..(py + 1) * w as usize];
		for (dst, src) in dst_row.chunks_exact_mut(4).zip(src_row.iter()) {
			dst[0] = src[0];
			dst[1] = src[1];
			dst[2] = src[2];
		}
	}
}

pub fn apply_pixelate(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, block_size: usize) {
	let block_size = block_size.max(1);
	let x_end = (x + w).min(img.width());
	let y_end = (y + h).min(img.height());
	if x >= x_end || y >= y_end {
		return;
	}

	let stride = img.width() as usize * 4;
	let step = block_size as u32;
	let copy_off = x as usize * 4;
	let copy_len = (x_end - x) as usize * 4;
	let raw = img.as_mut();

	for py in (y..y_end).step_by(block_size) {
		let py_end = (py + step).min(y_end);
		let first_row_start = py as usize * stride;
		{
			let first_row = &mut raw[first_row_start..first_row_start + stride];
			for px in (x..x_end).step_by(block_size) {
				let px_end = (px + step).min(x_end);
				let sample_idx = px as usize * 4;
				let r = first_row[sample_idx];
				let g = first_row[sample_idx + 1];
				let b = first_row[sample_idx + 2];
				let block_slice = &mut first_row[sample_idx..px_end as usize * 4];
				for pixel in block_slice.chunks_exact_mut(4) {
					pixel[0] = r;
					pixel[1] = g;
					pixel[2] = b;
				}
			}
		}

		let src_start = first_row_start + copy_off;
		let src_end = src_start + copy_len;
		for cy in (py + 1)..py_end {
			let dst_start = cy as usize * stride + copy_off;
			raw.copy_within(src_start..src_end, dst_start);
		}
	}
}

pub fn apply_highlight(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Color) {
	let x_end = (x + w).min(img.width());
	let y_end = (y + h).min(img.height());
	if x >= x_end || y >= y_end {
		return;
	}

	let alpha = color.a as u16;
	let a_inv = 255 - alpha;

	let r_blend = color.r as u16 * alpha;
	let g_blend = color.g as u16 * alpha;
	let b_blend = color.b as u16 * alpha;

	let stride = img.width() as usize * 4;
	let row_offset = x as usize * 4;
	let row_bytes = (x_end - x) as usize * 4;
	let raw = img.as_mut();

	for py in y..y_end {
		let start = py as usize * stride + row_offset;
		let row = &mut raw[start..start + row_bytes];
		for pixel in row.chunks_exact_mut(4) {
			pixel[0] = ((r_blend + pixel[0] as u16 * a_inv) / 255) as u8;
			pixel[1] = ((g_blend + pixel[1] as u16 * a_inv) / 255) as u8;
			pixel[2] = ((b_blend + pixel[2] as u16 * a_inv) / 255) as u8;
		}
	}
}

pub fn get_text_size(text: &str) -> (f64, f64) {
	thread_local! {
		static TEXT_LAYOUT: (ImageSurface, Context, pango::Layout) = {
			let surface = ImageSurface::create(Format::ARgb32, 1, 1).expect("failed to create surface");
			let cr = Context::new(&surface).expect("failed to create context");
			let layout = create_layout(&cr);
			let font = pango::FontDescription::from_string("system-ui Bold 20");
			layout.set_font_description(Some(&font));
			(surface, cr, layout)
		};
	}
	TEXT_LAYOUT.with(|(_, _, layout)| {
		layout.set_text(text);
		let (_, logical_rect) = layout.pixel_extents();
		(logical_rect.width() as f64, logical_rect.height() as f64)
	})
}

pub fn hit_test(ann: &Annotation, point: (f64, f64), threshold: f64) -> bool {
	ann.tool.behavior().hit_test(ann, point, threshold)
}

pub fn dist_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
	let dx = b.0 - a.0;
	let dy = b.1 - a.1;
	if dx == 0.0 && dy == 0.0 {
		let dpx = p.0 - a.0;
		let dpy = p.1 - a.1;
		return (dpx * dpx + dpy * dpy).sqrt();
	}
	let t = ((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy);
	let t = t.clamp(0.0, 1.0);
	let nearest_x = a.0 + t * dx;
	let nearest_y = a.1 + t * dy;
	let dpx = p.0 - nearest_x;
	let dpy = p.1 - nearest_y;
	(dpx * dpx + dpy * dpy).sqrt()
}

pub fn set_source_color(cr: &Context, color: crate::config::Color) {
	let (r, g, b, a) = color.components();
	cr.set_source_rgba(r, g, b, a);
}

pub fn draw_annotation(
	cr: &Context,
	ann: &Annotation,
	output: &OutputInfo,
	config: &SelectionConfig,
) {
	ann.tool.behavior().draw(cr, ann, output, config);
}
