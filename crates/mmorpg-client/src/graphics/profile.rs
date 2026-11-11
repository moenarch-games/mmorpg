//! Manual frame measurement for `outpost_frame_time` (#692): CPU spans around render-engine's
//! `prepare` and `encode`, their work counts, and each pass's GPU time from timestamp queries.
use crate::ClientError;
use render_engine::{
    renderer::{FrameTimestamps, PrepareWork, Viewport},
    world::WorldWork,
};
use std::time::Duration;

/// What the last [`super::frame::FrameRenderer::render`] did on the CPU.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameProfile {
    /// `Renderer::prepare`: culling, sorting and the frame's uploads.
    pub prepare: Duration,
    /// `Renderer::encode`: recording the passes.
    pub encode: Duration,
    /// What that `prepare` did.
    pub work: PrepareWork,
    /// The world's edit work so far (cumulative; subtract two readings).
    pub world: WorldWork,
}

/// Timestamp queries for one frame at a time, read back after it is submitted.
pub struct GpuTimer {
    query_set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    capacity: u32,
    labels: Vec<&'static str>,
}

impl GpuTimer {
    pub fn new(device: &wgpu::Device, capacity: u32) -> Self {
        let size = u64::from(capacity) * 8;
        Self {
            query_set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("frame timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: capacity,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("timestamp resolve"),
                size,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("timestamp readback"),
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            capacity,
            labels: Vec::new(),
        }
    }

    /// The queries `encode` writes for the frame last prepared into `viewport`.
    pub fn frame(&mut self, device: &wgpu::Device, viewport: &Viewport) -> FrameTimestamps<'_> {
        let queries = viewport.timestamp_queries();
        if queries > self.capacity {
            *self = Self::new(device, queries.next_power_of_two());
        }
        self.labels = viewport.pass_labels();
        FrameTimestamps {
            query_set: &self.query_set,
            first: 0,
        }
    }

    fn queries(&self) -> u32 {
        2 * self.labels.len() as u32
    }

    /// Copies the frame's timestamps for [`GpuTimer::read`]; after the frame's last pass.
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        let queries = self.queries();
        encoder.resolve_query_set(&self.query_set, 0..queries, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, u64::from(queries) * 8);
    }

    /// Each pass of the last submitted frame and its GPU time in milliseconds, in pass order
    /// (`Viewport::pass_labels`).
    pub fn read(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<Vec<(&'static str, f64)>, ClientError> {
        let slice = self.readback.slice(..u64::from(self.queries()) * 8);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = sender.send(r);
        });
        device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })?;
        receiver.recv_timeout(Duration::from_secs(10))??;
        let mapped = slice.get_mapped_range()?;
        let ticks: Vec<u64> = mapped
            .as_chunks::<8>()
            .0
            .iter()
            .map(|bytes| u64::from_le_bytes(*bytes))
            .collect();
        drop(mapped);
        self.readback.unmap();
        let period = f64::from(queue.get_timestamp_period());
        Ok(self
            .labels
            .iter()
            .zip(ticks.as_chunks::<2>().0)
            .map(|(label, [begin, end])| (*label, end.wrapping_sub(*begin) as f64 * period / 1e6))
            .collect())
    }
}
