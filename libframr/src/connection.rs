use anyhow::Result;
use wayland_client::globals::{registry_queue_init, GlobalList};
use wayland_client::Connection;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;

use crate::dispatch::{OutputEnumState, RegistryState};
use crate::error::FramrError;
use crate::output::{LogicalRegion, OutputInfo};

pub struct FramrConnection {
    pub(crate) conn: Connection,
    pub(crate) globals: GlobalList,
    outputs: Vec<OutputInfo>,
}

impl FramrConnection {
    pub fn new() -> Result<Self> {
        let conn = Connection::connect_to_env()
            .map_err(|e| FramrError::ConnectionFailed(format!("{e}")))?;
        let (globals, _event_queue) = registry_queue_init::<RegistryState>(&conn)
            .map_err(|e| FramrError::ConnectionFailed(format!("{e}")))?;
        let mut this = Self {
            conn,
            globals,
            outputs: Vec::new(),
        };
        this.refresh_outputs()?;
        Ok(this)
    }

    fn refresh_outputs(&mut self) -> Result<()> {
        let mut state = OutputEnumState::default();
        let mut event_queue = self.conn.new_event_queue::<OutputEnumState>();
        let qh = event_queue.handle();

        let _ = self.conn.display().get_registry(&qh, ());
        event_queue.roundtrip(&mut state)?;

        let Ok(xdg_mgr): Result<ZxdgOutputManagerV1, _> = self.globals.bind(&qh, 3..=3, ()) else {
            self.outputs = state.outputs.into_iter().map(Into::into).collect();
            return Ok(());
        };

        let xdg_outputs: Vec<_> = state
            .outputs
            .iter()
            .enumerate()
            .map(|(i, output)| xdg_mgr.get_xdg_output(&output.wl_output, &qh, i))
            .collect();
        event_queue.roundtrip(&mut state)?;

        for xdg in &xdg_outputs {
            xdg.destroy();
        }

        self.outputs = state.outputs.into_iter().map(Into::into).collect();

        if self.outputs.is_empty() {
            return Err(FramrError::NoOutputs.into());
        }

        Ok(())
    }

    pub fn get_all_outputs(&self) -> &[OutputInfo] {
        &self.outputs
    }

    pub fn get_output(&self, index: usize) -> Result<&OutputInfo> {
        self.outputs
            .get(index)
            .ok_or(FramrError::OutputNotFound(index).into())
    }

    pub fn screenshot_output(
        &self,
        output: &OutputInfo,
        include_cursor: bool,
    ) -> Result<image::RgbaImage> {
        crate::capture::capture_output(&self.conn, &self.globals, output, include_cursor)
    }

    pub fn screenshot_region(
        &self,
        output: &OutputInfo,
        region: &LogicalRegion,
        include_cursor: bool,
    ) -> Result<image::RgbaImage> {
        crate::capture::capture_region(&self.conn, &self.globals, output, region, include_cursor)
    }

    pub fn screenshot_all(&self, include_cursor: bool) -> Result<image::RgbaImage> {
        crate::capture::capture_all_outputs(
            &self.conn,
            &self.globals,
            &self.outputs,
            include_cursor,
        )
    }
}
