use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_output::Transform;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_client::globals::GlobalListContents;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::ZxdgOutputV1;
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1;
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;

use crate::output::{FrameFormat, OutputInfo, Position, Size};

pub struct RegistryState;

impl Dispatch<WlRegistry, GlobalListContents> for RegistryState {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

wayland_client::delegate_noop!(OutputEnumState: ignore ZxdgOutputManagerV1);

pub struct OutputEnumState {
    pub outputs: Vec<PartialOutput>,
}

pub struct PartialOutput {
    pub wl_output: WlOutput,
    pub name: String,
    pub description: String,
    pub physical_size: Size,
    pub transform: Transform,
    pub scale: i32,
    pub logical_position: Position,
    pub logical_size: Size,
}

impl Default for OutputEnumState {
    fn default() -> Self {
        Self {
            outputs: Vec::new(),
        }
    }
}

impl Dispatch<WlRegistry, ()> for OutputEnumState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: <WlRegistry as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_registry::Event as RegistryEvent;
        match event {
            RegistryEvent::Global {
                name,
                interface,
                version,
            } => {
                if interface == "wl_output" && version >= 2 {
                    let idx = state.outputs.len();
                    let output =
                        registry.bind::<WlOutput, _, Self>(name, version.min(4), qh, idx);
                    state.outputs.push(PartialOutput {
                        wl_output: output,
                        name: String::new(),
                        description: String::new(),
                        physical_size: Size {
                            width: 0,
                            height: 0,
                        },
                        transform: Transform::Normal,
                        scale: 1,
                        logical_position: Position { x: 0, y: 0 },
                        logical_size: Size {
                            width: 0,
                            height: 0,
                        },
                    });
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, usize> for OutputEnumState {
    fn event(
        state: &mut Self,
        _proxy: &WlOutput,
        event: <WlOutput as Proxy>::Event,
        index: &usize,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_output::Event as OutputEvent;
        let Some(output) = state.outputs.get_mut(*index) else {
            return;
        };
        match event {
            OutputEvent::Name { name } => {
                output.name = name;
            }
            OutputEvent::Description { description } => {
                output.description = description;
            }
            OutputEvent::Mode { width, height, .. } => {
                output.physical_size = Size {
                    width: width as u32,
                    height: height as u32,
                };
            }
            OutputEvent::Geometry { transform, .. } => {
                if let Ok(t) = transform.into_result() {
                    output.transform = t;
                }
            }
            OutputEvent::Scale { factor } => {
                output.scale = factor;
            }
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputV1, usize> for OutputEnumState {
    fn event(
        state: &mut Self,
        _proxy: &ZxdgOutputV1,
        event: <ZxdgOutputV1 as Proxy>::Event,
        index: &usize,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::Event as XdgOutputEvent;
        let Some(output) = state.outputs.get_mut(*index) else {
            return;
        };
        match event {
            XdgOutputEvent::LogicalPosition { x, y } => {
                output.logical_position = Position { x, y };
            }
            XdgOutputEvent::LogicalSize { width, height } => {
                output.logical_size = Size {
                    width: width as u32,
                    height: height as u32,
                };
            }
            _ => {}
        }
    }
}

impl From<PartialOutput> for OutputInfo {
    fn from(p: PartialOutput) -> Self {
        OutputInfo {
            name: p.name,
            description: p.description,
            logical_position: p.logical_position,
            logical_size: p.logical_size,
            physical_size: p.physical_size,
            transform: p.transform,
            scale: p.scale,
            wl_output: p.wl_output,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameState {
    Pending,
    Finished,
    Failed,
}

pub struct CaptureState {
    pub formats: Vec<FrameFormat>,
    pub buffer_done: bool,
    pub frame_state: FrameState,
    pub tv_sec_hi: u32,
    pub tv_sec_lo: u32,
    pub tv_nsec: u32,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            formats: Vec::new(),
            buffer_done: false,
            frame_state: FrameState::Pending,
            tv_sec_hi: 0,
            tv_sec_lo: 0,
            tv_nsec: 0,
        }
    }
}

wayland_client::delegate_noop!(CaptureState: ignore WlShm);
wayland_client::delegate_noop!(CaptureState: ignore WlShmPool);
wayland_client::delegate_noop!(CaptureState: ignore WlBuffer);
wayland_client::delegate_noop!(CaptureState: ignore WlRegistry);
wayland_client::delegate_noop!(CaptureState: ignore ZwlrScreencopyManagerV1);

impl Dispatch<ZwlrScreencopyFrameV1, ()> for CaptureState {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrScreencopyFrameV1,
        event: <ZwlrScreencopyFrameV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::Event as FrameEvent;
        match event {
            FrameEvent::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let Ok(fmt) = format.into_result() else { return };
                state.formats.push(FrameFormat {
                    format: fmt,
                    width: width as i32,
                    height: height as i32,
                    stride: stride as i32,
                });
            }
            FrameEvent::BufferDone => {
                state.buffer_done = true;
            }
            FrameEvent::Ready {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
            } => {
                state.tv_sec_hi = tv_sec_hi;
                state.tv_sec_lo = tv_sec_lo;
                state.tv_nsec = tv_nsec;
                state.frame_state = FrameState::Finished;
            }
            FrameEvent::Failed => {
                state.frame_state = FrameState::Failed;
            }
            _ => {}
        }
    }
}

pub struct CaptureSlot {
    pub formats: Vec<FrameFormat>,
    pub buffer_done: bool,
    pub frame_state: FrameState,
}

impl CaptureSlot {
    fn new() -> Self {
        Self {
            formats: Vec::new(),
            buffer_done: false,
            frame_state: FrameState::Pending,
        }
    }
}

pub struct MultiCaptureState {
    pub slots: Vec<CaptureSlot>,
}

impl MultiCaptureState {
    pub fn new(count: usize) -> Self {
        Self {
            slots: (0..count).map(|_| CaptureSlot::new()).collect(),
        }
    }

    pub fn all_buffer_done(&self) -> bool {
        self.slots.iter().all(|s| s.buffer_done)
    }

    pub fn all_finished(&self) -> bool {
        self.slots
            .iter()
            .all(|s| s.frame_state != FrameState::Pending)
    }
}

wayland_client::delegate_noop!(MultiCaptureState: ignore WlShm);
wayland_client::delegate_noop!(MultiCaptureState: ignore WlShmPool);
wayland_client::delegate_noop!(MultiCaptureState: ignore WlBuffer);
wayland_client::delegate_noop!(MultiCaptureState: ignore WlRegistry);
wayland_client::delegate_noop!(MultiCaptureState: ignore ZwlrScreencopyManagerV1);

impl Dispatch<ZwlrScreencopyFrameV1, usize> for MultiCaptureState {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrScreencopyFrameV1,
        event: <ZwlrScreencopyFrameV1 as Proxy>::Event,
        index: &usize,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::Event as FrameEvent;
        let Some(slot) = state.slots.get_mut(*index) else {
            return;
        };
        match event {
            FrameEvent::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let Ok(fmt) = format.into_result() else { return };
                slot.formats.push(FrameFormat {
                    format: fmt,
                    width: width as i32,
                    height: height as i32,
                    stride: stride as i32,
                });
            }
            FrameEvent::BufferDone => {
                slot.buffer_done = true;
            }
            FrameEvent::Ready { .. } => {
                slot.frame_state = FrameState::Finished;
            }
            FrameEvent::Failed => {
                slot.frame_state = FrameState::Failed;
            }
            _ => {}
        }
    }
}
