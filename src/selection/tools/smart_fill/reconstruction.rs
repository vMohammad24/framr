use libframr::OutputInfo;

use crate::config::Color;
use crate::selection::state::{BgraImage, SelectionRegion, SmartFillPattern};

const RING_WIDTH: i64 = 3;
const NEIGHBORHOOD_RADIUS: i64 = 3;
const LOCAL_SAMPLE_CAPACITY: usize = ((NEIGHBORHOOD_RADIUS * 2 + 1) * RING_WIDTH) as usize;
const SOLID_DISTANCE_SQUARED: u64 = 6 * 6 * 3;
const EDGE_SAMPLES: u32 = 128;

#[derive(Clone, Copy, Default)]
struct LinearColor {
	r: f32,
	g: f32,
	b: f32,
}

impl LinearColor {
	fn from_color(color: Color) -> Self {
		fn linear(channel: u8) -> f32 {
			let value = channel as f32 / 255.0;
			if value <= 0.04045 {
				value / 12.92
			} else {
				((value + 0.055) / 1.055).powf(2.4)
			}
		}

		Self {
			r: linear(color.r),
			g: linear(color.g),
			b: linear(color.b),
		}
	}

	fn to_color(self) -> Color {
		fn srgb(channel: f32) -> u8 {
			let value = channel.clamp(0.0, 1.0);
			let value = if value <= 0.0031308 {
				value * 12.92
			} else {
				1.055 * value.powf(1.0 / 2.4) - 0.055
			};
			(value * 255.0).round() as u8
		}

		Color {
			r: srgb(self.r),
			g: srgb(self.g),
			b: srgb(self.b),
			a: u8::MAX,
		}
	}

	fn lerp(self, other: Self, amount: f32) -> Self {
		Self {
			r: self.r + (other.r - self.r) * amount,
			g: self.g + (other.g - self.g) * amount,
			b: self.b + (other.b - self.b) * amount,
		}
	}

	fn add(self, other: Self) -> Self {
		Self {
			r: self.r + other.r,
			g: self.g + other.g,
			b: self.b + other.b,
		}
	}

	fn subtract(self, other: Self) -> Self {
		Self {
			r: self.r - other.r,
			g: self.g - other.g,
			b: self.b - other.b,
		}
	}
}

fn sample_pixel(
	sources: &[(OutputInfo, BgraImage)],
	global_x: i64,
	global_y: i64,
) -> Option<[u8; 3]> {
	for (output, image) in sources {
		let image = &image.0;
		let local_x = global_x - output.logical_position.x as i64;
		let local_y = global_y - output.logical_position.y as i64;
		if local_x >= 0
			&& local_y >= 0
			&& local_x < image.width() as i64
			&& local_y < image.height() as i64
		{
			let pixel = image.get_pixel(local_x as u32, local_y as u32);
			return Some([pixel[2], pixel[1], pixel[0]]);
		}
	}
	None
}

fn dominant_sample<I>(sources: &[(OutputInfo, BgraImage)], coordinates: I) -> Option<LinearColor>
where
	I: IntoIterator<Item = (i64, i64)>,
{
	let mut samples = [(0_u16, [0_u8; 3]); LOCAL_SAMPLE_CAPACITY];
	let mut count = 0;
	for (x, y) in coordinates {
		if count >= LOCAL_SAMPLE_CAPACITY {
			break;
		}
		if let Some(rgb) = sample_pixel(sources, x, y) {
			let cluster =
				((rgb[0] as u16 >> 3) << 10) | ((rgb[1] as u16 >> 3) << 5) | (rgb[2] as u16 >> 3);
			samples[count] = (cluster, rgb);
			count += 1;
		}
	}
	if count == 0 {
		return None;
	}

	samples[..count].sort_unstable_by_key(|sample| sample.0);
	let mut best = (0, 1);
	let mut start = 0;
	while start < count {
		let mut end = start + 1;
		while end < count && samples[end].0 == samples[start].0 {
			end += 1;
		}
		if end - start > best.1 - best.0 {
			best = (start, end);
		}
		start = end;
	}

	let dominant = &samples[best.0..best.1];
	let mut channels = [[0_u8; LOCAL_SAMPLE_CAPACITY]; 3];
	for (index, (_, rgb)) in dominant.iter().enumerate() {
		channels[0][index] = rgb[0];
		channels[1][index] = rgb[1];
		channels[2][index] = rgb[2];
	}
	for channel in &mut channels {
		channel[..dominant.len()].sort_unstable();
	}
	let middle = dominant.len() / 2;
	let color = Color {
		r: channels[0][middle],
		g: channels[1][middle],
		b: channels[2][middle],
		a: u8::MAX,
	};
	Some(LinearColor::from_color(color))
}

fn positions(start: i64, end: i64, count: usize) -> Vec<i64> {
	if count <= 1 {
		return vec![start];
	}
	let span = (end - start - 1).max(0) as usize;
	(0..count)
		.map(|index| start + (index * span / (count - 1)) as i64)
		.collect()
}

fn horizontal_edge(
	sources: &[(OutputInfo, BgraImage)],
	positions: &[i64],
	y_start: i64,
	y_end: i64,
) -> Vec<Option<LinearColor>> {
	positions
		.iter()
		.map(|&x| {
			dominant_sample(
				sources,
				(x - NEIGHBORHOOD_RADIUS..=x + NEIGHBORHOOD_RADIUS).flat_map(|sample_x| {
					(y_start..y_end).map(move |sample_y| (sample_x, sample_y))
				}),
			)
		})
		.collect()
}

fn vertical_edge(
	sources: &[(OutputInfo, BgraImage)],
	positions: &[i64],
	x_start: i64,
	x_end: i64,
) -> Vec<Option<LinearColor>> {
	positions
		.iter()
		.map(|&y| {
			dominant_sample(
				sources,
				(x_start..x_end).flat_map(|sample_x| {
					(y - NEIGHBORHOOD_RADIUS..=y + NEIGHBORHOOD_RADIUS)
						.map(move |sample_y| (sample_x, sample_y))
				}),
			)
		})
		.collect()
}

fn repair_edge(samples: Vec<Option<LinearColor>>) -> Option<Vec<LinearColor>> {
	if !samples.iter().any(Option::is_some) {
		return None;
	}

	let mut repaired = Vec::with_capacity(samples.len());
	let mut index = 0;
	while index < samples.len() {
		if let Some(sample) = samples[index] {
			repaired.push(sample);
			index += 1;
			continue;
		}

		let gap_start = index;
		while index < samples.len() && samples[index].is_none() {
			index += 1;
		}
		let before = repaired.last().copied();
		let after = samples.get(index).copied().flatten();
		for gap_index in gap_start..index {
			let color = match (before, after) {
				(Some(before), Some(after)) => before.lerp(
					after,
					(gap_index - gap_start + 1) as f32 / (index - gap_start + 1) as f32,
				),
				(Some(nearest), None) | (None, Some(nearest)) => nearest,
				(None, None) => unreachable!("at least one edge sample exists"),
			};
			repaired.push(color);
		}
	}
	Some(repaired)
}

fn synthetic_edge(
	length: usize,
	start: Option<LinearColor>,
	end: Option<LinearColor>,
	fallback: LinearColor,
) -> Vec<LinearColor> {
	let start = start.unwrap_or(fallback);
	let end = end.unwrap_or(start);
	if length <= 1 {
		return vec![start];
	}
	(0..length)
		.map(|index| start.lerp(end, index as f32 / (length - 1) as f32))
		.collect()
}

fn reconcile_corner(first: &mut LinearColor, second: &mut LinearColor) {
	*first = first.lerp(*second, 0.5);
	*second = *first;
}

fn edge_at(edge: &[LinearColor], amount: f32) -> LinearColor {
	if edge.len() == 1 {
		return edge[0];
	}
	let position = amount.clamp(0.0, 1.0) * (edge.len() - 1) as f32;
	let first = position.floor() as usize;
	let second = (first + 1).min(edge.len() - 1);
	edge[first].lerp(edge[second], position - first as f32)
}

fn color_distance(first: Color, second: Color) -> u64 {
	let dr = first.r as i64 - second.r as i64;
	let dg = first.g as i64 - second.g as i64;
	let db = first.b as i64 - second.b as i64;
	(dr * dr + dg * dg + db * db) as u64
}

fn solid_color(edges: [&[LinearColor]; 4]) -> Option<Color> {
	let colors: Vec<Color> = edges
		.into_iter()
		.flatten()
		.map(|sample| sample.to_color())
		.collect();
	let mut red: Vec<u8> = colors.iter().map(|color| color.r).collect();
	let mut green: Vec<u8> = colors.iter().map(|color| color.g).collect();
	let mut blue: Vec<u8> = colors.iter().map(|color| color.b).collect();
	red.sort_unstable();
	green.sort_unstable();
	blue.sort_unstable();
	let middle = colors.len() / 2;
	let median = Color {
		r: red[middle],
		g: green[middle],
		b: blue[middle],
		a: u8::MAX,
	};
	let matching = colors
		.iter()
		.filter(|color| color_distance(**color, median) <= SOLID_DISTANCE_SQUARED)
		.count();
	(matching * 100 >= colors.len() * 95).then_some(median)
}

fn dimensions(width: u32, height: u32) -> (u32, u32) {
	let longest = width.max(height).max(1);
	if longest <= EDGE_SAMPLES {
		return (width.max(2), height.max(2));
	}

	let scaled_width = (width as u64 * EDGE_SAMPLES as u64 + longest as u64 / 2) / longest as u64;
	let scaled_height = (height as u64 * EDGE_SAMPLES as u64 + longest as u64 / 2) / longest as u64;
	(scaled_width.max(2) as u32, scaled_height.max(2) as u32)
}

pub(super) fn reconstruct(
	region: SelectionRegion,
	sources: &[(OutputInfo, BgraImage)],
	fallback: Color,
) -> (SmartFillPattern, Color) {
	let (left, top, right, bottom) = region.integer_bounds();
	let (field_width, field_height) = dimensions((right - left) as u32, (bottom - top) as u32);
	let x_positions = positions(left, right, field_width as usize);
	let y_positions = positions(top, bottom, field_height as usize);

	let mut top_edge = repair_edge(horizontal_edge(
		sources,
		&x_positions,
		top - RING_WIDTH,
		top,
	));
	let mut bottom_edge = repair_edge(horizontal_edge(
		sources,
		&x_positions,
		bottom,
		bottom + RING_WIDTH,
	));
	let mut left_edge = repair_edge(vertical_edge(
		sources,
		&y_positions,
		left - RING_WIDTH,
		left,
	));
	let mut right_edge = repair_edge(vertical_edge(
		sources,
		&y_positions,
		right,
		right + RING_WIDTH,
	));

	let fallback = LinearColor::from_color(fallback);
	if top_edge.is_none() {
		top_edge = Some(synthetic_edge(
			field_width as usize,
			left_edge.as_ref().and_then(|edge| edge.first()).copied(),
			right_edge.as_ref().and_then(|edge| edge.first()).copied(),
			fallback,
		));
	}
	if bottom_edge.is_none() {
		bottom_edge = Some(synthetic_edge(
			field_width as usize,
			left_edge.as_ref().and_then(|edge| edge.last()).copied(),
			right_edge.as_ref().and_then(|edge| edge.last()).copied(),
			fallback,
		));
	}
	if left_edge.is_none() {
		left_edge = Some(synthetic_edge(
			field_height as usize,
			top_edge.as_ref().and_then(|edge| edge.first()).copied(),
			bottom_edge.as_ref().and_then(|edge| edge.first()).copied(),
			fallback,
		));
	}
	if right_edge.is_none() {
		right_edge = Some(synthetic_edge(
			field_height as usize,
			top_edge.as_ref().and_then(|edge| edge.last()).copied(),
			bottom_edge.as_ref().and_then(|edge| edge.last()).copied(),
			fallback,
		));
	}

	let mut top_edge = top_edge.expect("missing top edge is synthesized");
	let mut bottom_edge = bottom_edge.expect("missing bottom edge is synthesized");
	let mut left_edge = left_edge.expect("missing left edge is synthesized");
	let mut right_edge = right_edge.expect("missing right edge is synthesized");
	reconcile_corner(&mut top_edge[0], &mut left_edge[0]);
	reconcile_corner(
		top_edge.last_mut().expect("edge is non-empty"),
		&mut right_edge[0],
	);
	reconcile_corner(
		&mut bottom_edge[0],
		left_edge.last_mut().expect("edge is non-empty"),
	);
	reconcile_corner(
		bottom_edge.last_mut().expect("edge is non-empty"),
		right_edge.last_mut().expect("edge is non-empty"),
	);

	if let Some(color) = solid_color([&top_edge, &bottom_edge, &left_edge, &right_edge]) {
		return (SmartFillPattern::Solid(color), color);
	}

	let top_left = top_edge[0];
	let top_right = *top_edge.last().expect("edge is non-empty");
	let bottom_left = bottom_edge[0];
	let bottom_right = *bottom_edge.last().expect("edge is non-empty");
	let mut pixels = Vec::with_capacity((field_width * field_height) as usize);
	for y in 0..field_height {
		let v = y as f32 / (field_height - 1) as f32;
		for x in 0..field_width {
			let u = x as f32 / (field_width - 1) as f32;
			let top = edge_at(&top_edge, u);
			let bottom = edge_at(&bottom_edge, u);
			let left = edge_at(&left_edge, v);
			let right = edge_at(&right_edge, v);
			let vertical = top.lerp(bottom, v);
			let horizontal = left.lerp(right, u);
			let corners = top_left
				.lerp(top_right, u)
				.lerp(bottom_left.lerp(bottom_right, u), v);
			pixels.push(vertical.add(horizontal).subtract(corners).to_color());
		}
	}

	let representative = pixels[pixels.len() / 2];
	(
		SmartFillPattern::Field {
			width: field_width,
			height: field_height,
			pixels,
		},
		representative,
	)
}

pub(super) fn sample_pattern(pattern: &SmartFillPattern, u: f32, v: f32, fallback: Color) -> Color {
	let SmartFillPattern::Field {
		width,
		height,
		pixels,
	} = pattern
	else {
		return match pattern {
			SmartFillPattern::Solid(color) => *color,
			_ => fallback,
		};
	};
	if *width == 0 || *height == 0 || pixels.len() != (*width * *height) as usize {
		return fallback;
	}

	let x = u.clamp(0.0, 1.0) * width.saturating_sub(1) as f32;
	let y = v.clamp(0.0, 1.0) * height.saturating_sub(1) as f32;
	let x0 = x.floor() as u32;
	let y0 = y.floor() as u32;
	let x1 = (x0 + 1).min(width - 1);
	let y1 = (y0 + 1).min(height - 1);
	let fx = x - x0 as f32;
	let fy = y - y0 as f32;
	let color = |sample_x: u32, sample_y: u32| pixels[(sample_y * *width + sample_x) as usize];
	let interpolate = |first: Color, second: Color, amount: f32| Color {
		r: (first.r as f32 + (second.r as f32 - first.r as f32) * amount).round() as u8,
		g: (first.g as f32 + (second.g as f32 - first.g as f32) * amount).round() as u8,
		b: (first.b as f32 + (second.b as f32 - first.b as f32) * amount).round() as u8,
		a: u8::MAX,
	};
	let top = interpolate(color(x0, y0), color(x1, y0), fx);
	let bottom = interpolate(color(x0, y1), color(x1, y1), fx);
	interpolate(top, bottom, fy)
}
