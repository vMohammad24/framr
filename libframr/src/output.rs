use std::fmt;

use wayland_client::protocol::wl_output::Transform;
use wayland_client::protocol::wl_shm::Format;
use wayland_client::protocol::wl_output::WlOutput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogicalRegion {
    pub position: Position,
    pub size: Size,
}

impl LogicalRegion {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            position: Position { x, y },
            size: Size { width, height },
        }
    }
}

#[derive(Debug, Clone)]
pub struct OutputInfo {
    pub name: String,
    pub description: String,
    pub logical_position: Position,
    pub logical_size: Size,
    pub physical_size: Size,
    pub transform: Transform,
    pub scale: i32,
    pub(crate) wl_output: WlOutput,
}

impl fmt::Display for OutputInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}x{}+{}+{})",
            self.name,
            self.logical_size.width,
            self.logical_size.height,
            self.logical_position.x,
            self.logical_position.y
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FrameFormat {
    pub format: Format,
    pub width: i32,
    pub height: i32,
    pub stride: i32,
}

impl FrameFormat {
    pub fn byte_size(&self) -> usize {
        (self.stride * self.height) as usize
    }
}
