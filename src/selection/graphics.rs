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
	let mut cairo_data = img.as_raw().clone();
	for pixel in cairo_data.chunks_exact_mut(4) {
		pixel.swap(0, 2); // RGBA to BGRA
	}

	let mut surface = ImageSurface::create_for_data(
		cairo_data,
		Format::ARgb32,
		w as i32,
		h as i32,
		(w * 4) as i32,
	)
	.map_err(|e| anyhow!("failed to create cairo surface: {e}"))?;
	{
		let cr = Context::new(&surface).map_err(|e| anyhow!("failed to create context: {e}"))?;
		for ann in annotations {
			if !ann.tool.behavior().is_region_effect() {
				draw_annotation(&cr, ann, output, config);
			}
		}
	}

	surface.flush();
	let data = surface
		.data()
		.map_err(|e| anyhow!("failed to get surface data: {e}"))?;
	for (dst, src) in img.as_mut().chunks_exact_mut(4).zip(data.chunks_exact(4)) {
		dst[0] = src[2];
		dst[1] = src[1];
		dst[2] = src[0];
		dst[3] = src[3];
	}
	Ok(())
}

pub fn apply_blur(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, radius: f32) {
	let mut data = vec![[0u8; 3]; (w * h) as usize];
	let img_w = img.width();
	let raw = img.as_raw();

	for py in 0..h {
		for px in 0..w {
			let img_idx = (((y + py) * img_w + (x + px)) * 4) as usize;
			let data_idx = (py * w + px) as usize;
			if img_idx + 2 < raw.len() {
				data[data_idx] = [raw[img_idx], raw[img_idx + 1], raw[img_idx + 2]];
			}
		}
	}

	fastblur::gaussian_blur(&mut data, w as usize, h as usize, radius);

	let raw_mut = img.as_mut();
	for py in 0..h {
		for px in 0..w {
			let img_idx = (((y + py) * img_w + (x + px)) * 4) as usize;
			let data_idx = (py * w + px) as usize;
			if img_idx + 2 < raw_mut.len() {
				let p = data[data_idx];
				raw_mut[img_idx] = p[0];
				raw_mut[img_idx + 1] = p[1];
				raw_mut[img_idx + 2] = p[2];
			}
		}
	}
}

pub fn apply_pixelate(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, block_size: usize) {
	let img_w = img.width();
	let raw = img.as_mut();

	for py in (y..y + h).step_by(block_size) {
		for px in (x..x + w).step_by(block_size) {
			let idx = ((py * img_w + px) * 4) as usize;
			if idx + 2 < raw.len() {
				let r = raw[idx];
				let g = raw[idx + 1];
				let b = raw[idx + 2];

				for by in 0..block_size {
					for bx in 0..block_size {
						let cx = px + bx as u32;
						let cy = py + by as u32;
						if cx < x + w && cy < y + h {
							let c_idx = ((cy * img_w + cx) * 4) as usize;
							if c_idx + 2 < raw.len() {
								raw[c_idx] = r;
								raw[c_idx + 1] = g;
								raw[c_idx + 2] = b;
							}
						}
					}
				}
			}
		}
	}
}

pub fn apply_highlight(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Color) {
	let alpha = color.a as u16;
	let a_inv = 255 - alpha;

	let r_blend = color.r as u16 * alpha;
	let g_blend = color.g as u16 * alpha;
	let b_blend = color.b as u16 * alpha;

	for py in y..y + h {
		for px in x..x + w {
			let pixel = img.get_pixel_mut(px, py);

			pixel[0] = ((r_blend + pixel[0] as u16 * a_inv) / 255) as u8;
			pixel[1] = ((g_blend + pixel[1] as u16 * a_inv) / 255) as u8;
			pixel[2] = ((b_blend + pixel[2] as u16 * a_inv) / 255) as u8;
		}
	}
}

pub fn get_text_size(text: &str) -> (f64, f64) {
	let surface = ImageSurface::create(Format::ARgb32, 1, 1).expect("failed to create surface");
	let cr = Context::new(&surface).expect("failed to create context");
	let layout = create_layout(&cr);
	layout.set_text(text);
	let font = pango::FontDescription::from_string("system-ui Bold 20");
	layout.set_font_description(Some(&font));
	let (_, logical_rect) = layout.pixel_extents();
	(logical_rect.width() as f64, logical_rect.height() as f64)
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
