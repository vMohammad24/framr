use cairo::{Antialias, Context, Format, ImageSurface, LineCap, LineJoin};
use image::{Rgba, RgbaImage};
use libframr::OutputInfo;
use pangocairo::functions::{create_layout, show_layout};

use crate::selection::state::{Annotation, Tool};

pub fn apply_annotations(img: &mut RgbaImage, annotations: &[Annotation], output: &OutputInfo) {
	for ann in annotations {
		if (ann.tool == Tool::Blur || ann.tool == Tool::Pixelate) && ann.points.len() >= 2 {
			let (x1, y1) = (
				ann.points[0].0 - output.logical_position.x as f64,
				ann.points[0].1 - output.logical_position.y as f64,
			);
			let (x2, y2) = (
				ann.points[1].0 - output.logical_position.x as f64,
				ann.points[1].1 - output.logical_position.y as f64,
			);
			let bx = (x1.min(x2) as u32).min(img.width());
			let by = (y1.min(y2) as u32).min(img.height());
			let bw = ((x1 - x2).abs() as u32).min(img.width() - bx);
			let bh = ((y1 - y2).abs() as u32).min(img.height() - by);
			if bw > 0 && bh > 0 {
				if ann.tool == Tool::Blur {
					apply_blur(img, bx, by, bw, bh);
				} else {
					apply_pixelate(img, bx, by, bw, bh);
				}
			}
		}
	}

	let (w, h) = img.dimensions();
	let mut cairo_data = Vec::with_capacity((w * h * 4) as usize);
	for pixel in img.pixels() {
		// BGRA
		cairo_data.push(pixel[2]);
		cairo_data.push(pixel[1]);
		cairo_data.push(pixel[0]);
		cairo_data.push(pixel[3]);
	}

	let mut surface =
		ImageSurface::create(Format::ARgb32, w as i32, h as i32).expect("failed to create surface");
	{
		let cr = Context::new(&surface).expect("failed to create context");
		let src_surface = ImageSurface::create_for_data(
			cairo_data,
			Format::ARgb32,
			w as i32,
			h as i32,
			(w * 4) as i32,
		)
		.expect("failed to create src surface");
		if let Err(e) = cr.set_source_surface(&src_surface, 0.0, 0.0) {
			eprintln!("failed to set source surface: {}", e);
		}
		if let Err(e) = cr.paint() {
			eprintln!("failed to paint: {}", e);
		}
		for ann in annotations {
			if ann.tool != Tool::Blur && ann.tool != Tool::Pixelate {
				draw_annotation(&cr, ann, output);
			}
		}
	}

	surface.flush();
	let data = surface.data().expect("failed to get surface data");
	for (i, chunk) in data.chunks(4).enumerate() {
		let py = i as u32 / w;
		let px = i as u32 % w;
		if py < h && px < w {
			img.put_pixel(px, py, Rgba([chunk[2], chunk[1], chunk[0], chunk[3]]));
		}
	}
}

pub fn apply_blur(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32) {
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

	fastblur::gaussian_blur(&mut data, w as usize, h as usize, 10.0);

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

pub fn apply_pixelate(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32) {
	let block_size = 10;
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

pub fn draw_annotation(cr: &Context, ann: &Annotation, output: &OutputInfo) {
	cr.set_source_rgb(ann.color.0, ann.color.1, ann.color.2);
	cr.set_line_width(4.0);
	cr.set_antialias(Antialias::Best);
	cr.set_line_cap(LineCap::Round);
	cr.set_line_join(LineJoin::Round);

	let offset_x = output.logical_position.x as f64;
	let offset_y = output.logical_position.y as f64;

	match ann.tool {
		Tool::Circle => {
			if ann.points.len() >= 2 {
				let (x1, y1) = (ann.points[0].0 - offset_x, ann.points[0].1 - offset_y);
				let (x2, y2) = (ann.points[1].0 - offset_x, ann.points[1].1 - offset_y);
				let dx = x1 - x2;
				let dy = y1 - y2;
				let radius = (dx * dx + dy * dy).sqrt();

				cr.arc(x1, y1, radius, 0.0, 2.0 * std::f64::consts::PI);
				if let Err(e) = cr.stroke() {
					eprintln!("failed to stroke circle: {}", e);
				}
			}
		}
		Tool::Arrow => {
			if ann.points.len() >= 2 {
				let (x1, y1) = (ann.points[0].0 - offset_x, ann.points[0].1 - offset_y);
				let (x2, y2) = (ann.points[1].0 - offset_x, ann.points[1].1 - offset_y);
				cr.move_to(x1, y1);
				cr.line_to(x2, y2);
				if let Err(e) = cr.stroke() {
					eprintln!("failed to stroke arrow line: {}", e);
				}
				let angle = (y2 - y1).atan2(x2 - x1);
				let head_len = 20.0;
				cr.move_to(x2, y2);
				cr.line_to(
					x2 - head_len * (angle - 0.5).cos(),
					y2 - head_len * (angle - 0.5).sin(),
				);
				cr.move_to(x2, y2);
				cr.line_to(
					x2 - head_len * (angle + 0.5).cos(),
					y2 - head_len * (angle + 0.5).sin(),
				);
				if let Err(e) = cr.stroke() {
					eprintln!("failed to stroke arrow head: {}", e);
				}
			}
		}
		Tool::Checkmark => {
			if !ann.points.is_empty() {
				let (x, y) = (ann.points[0].0 - offset_x, ann.points[0].1 - offset_y);
				cr.move_to(x - 10.0, y);
				cr.line_to(x, y + 10.0);
				cr.line_to(x + 20.0, y - 10.0);
				if let Err(e) = cr.stroke() {
					eprintln!("failed to stroke checkmark: {}", e);
				}
			}
		}
		Tool::Annotate => {
			if !ann.points.is_empty() {
				let (x0, y0) = (ann.points[0].0 - offset_x, ann.points[0].1 - offset_y);
				cr.move_to(x0, y0);
				for p in &ann.points[1..] {
					cr.line_to(p.0 - offset_x, p.1 - offset_y);
				}
				if let Err(e) = cr.stroke() {
					eprintln!("failed to stroke annotation: {}", e);
				}
			}
		}
		Tool::Text => {
			if let Some(text) = &ann.text
				&& !ann.points.is_empty()
			{
				let (x, y) = (ann.points[0].0 - offset_x, ann.points[0].1 - offset_y);
				let layout = create_layout(cr);
				layout.set_text(text);
				let font = pango::FontDescription::from_string("system-ui Bold 20");
				layout.set_font_description(Some(&font));
				cr.move_to(x, y);
				show_layout(cr, &layout);
			}
		}
		_ => {}
	}
}
