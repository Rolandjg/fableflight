//! gpu time per render stage with FABLE_GPU_TIMES=1 (waits on the gpu, profiling only),
//! and per frame without waiting, for dynamic resolution.

const SLOTS: u32 = 32;

pub struct GpuTimer {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    period: f64,
    labels: Vec<&'static str>,
    acc: Vec<(&'static str, f64)>,
    frames: u32,
    inside_passes: bool,
}

impl GpuTimer {
    /// timestamp features if the adapter has them, inside passes too when stage timing is on.
    pub fn features(adapter: &wgpu::Adapter) -> wgpu::Features {
        let f = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        if !adapter.features().contains(f) {
            return wgpu::Features::empty();
        }
        if !Self::wanted() {
            return f;
        }
        f | (adapter.features() & wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES)
    }

    fn wanted() -> bool {
        std::env::var("FABLE_GPU_TIMES").is_ok_and(|v| v == "1")
    }

    pub fn new(d: &wgpu::Device, q: &wgpu::Queue) -> Option<Self> {
        if !Self::wanted() || !d.features().contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) {
            return None;
        }
        let size = SLOTS as u64 * 8;
        let set = d.create_query_set(&wgpu::QuerySetDescriptor { label: Some("gpu timer"), ty: wgpu::QueryType::Timestamp, count: SLOTS });
        let resolve = d.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let read = d.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        log::info!("gpu stage timing on");
        Some(Self { set, resolve, read, period: q.get_timestamp_period() as f64, labels: Vec::new(), acc: Vec::new(), frames: 0, inside_passes: d.features().contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES) })
    }

    /// stamp the end of a stage.
    pub fn mark(&mut self, enc: &mut wgpu::CommandEncoder, label: &'static str) {
        if (self.labels.len() as u32) < SLOTS {
            enc.write_timestamp(&self.set, self.labels.len() as u32);
            self.labels.push(label);
        }
    }

    /// stamp the end of a stage inside a render pass.
    pub fn mark_pass(&mut self, rp: &mut wgpu::RenderPass, label: &'static str) {
        if self.inside_passes && (self.labels.len() as u32) < SLOTS {
            rp.write_timestamp(&self.set, self.labels.len() as u32);
            self.labels.push(label);
        }
    }

    /// copy the stamps out before submit.
    pub fn resolve(&mut self, enc: &mut wgpu::CommandEncoder) {
        let n = self.labels.len() as u32;
        if n > 1 {
            enc.resolve_query_set(&self.set, 0..n, &self.resolve, 0);
            enc.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, n as u64 * 8);
        }
    }

    /// read the stamps after submit, this waits for the gpu so it is for profiling only.
    pub fn collect(&mut self, d: &wgpu::Device) {
        let n = self.labels.len();
        if n > 1 {
            self.read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            let _ = d.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
            {
                let data = self.read.slice(..).get_mapped_range().expect("mapped");
                let ts: &[u64] = bytemuck::cast_slice(&data[..n * 8]);
                for i in 1..n {
                    let ms = ts[i].saturating_sub(ts[i - 1]) as f64 * self.period / 1e6;
                    match self.acc.iter_mut().find(|a| a.0 == self.labels[i]) {
                        Some(a) => a.1 += ms,
                        None => self.acc.push((self.labels[i], ms)),
                    }
                }
            }
            self.read.unmap();
            self.frames += 1;
        }
        self.labels.clear();
        if self.frames >= 60 {
            let k = 1.0 / self.frames as f64;
            let s: Vec<String> = self.acc.iter().map(|(l, ms)| format!("{l} {:.1}", ms * k)).collect();
            log::info!("gpu ms: {}", s.join(", "));
            self.acc.clear();
            self.frames = 0;
        }
    }
}

/// frames in flight for the frame timer.
const FRAMES: usize = 4;
/// readback slot states.
const FREE: u8 = 0;
const PENDING: u8 = 1;
const MAPPED: u8 = 2;

/// gpu time of whole frames, read back a few frames late so nothing waits.
pub struct FrameTimer {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    reads: Vec<(wgpu::Buffer, std::sync::Arc<std::sync::atomic::AtomicU8>)>,
    period: f64,
    /// slot this frame stamps into, if free
    cur: Option<usize>,
    next: usize,
    /// latest frame gpu time in ms, and frames timed so far
    pub last_ms: Option<f64>,
    pub frames: u64,
}

impl FrameTimer {
    pub fn new(d: &wgpu::Device, q: &wgpu::Queue) -> Option<Self> {
        if !d.features().contains(wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) {
            return None;
        }
        let n = FRAMES as u64 * 2;
        let set = d.create_query_set(&wgpu::QuerySetDescriptor { label: Some("frame timer"), ty: wgpu::QueryType::Timestamp, count: n as u32 });
        // resolve offsets must be 256 aligned
        let resolve = d.create_buffer(&wgpu::BufferDescriptor { label: Some("frame timer resolve"), size: FRAMES as u64 * wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let reads = (0..FRAMES)
            .map(|_| (d.create_buffer(&wgpu::BufferDescriptor { label: Some("frame timer read"), size: 16, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false }), std::sync::Arc::new(std::sync::atomic::AtomicU8::new(FREE))))
            .collect();
        Some(Self { set, resolve, reads, period: q.get_timestamp_period() as f64, cur: None, next: 0, last_ms: None, frames: 0 })
    }

    /// stamp the frame start, skipped while the slot is still being read back.
    pub fn begin(&mut self, enc: &mut wgpu::CommandEncoder) {
        use std::sync::atomic::Ordering;
        let i = self.next;
        self.cur = (self.reads[i].1.load(Ordering::Acquire) == FREE).then_some(i);
        if let Some(i) = self.cur {
            enc.write_timestamp(&self.set, i as u32 * 2);
        }
    }

    /// stamp the frame end and copy out.
    pub fn end(&mut self, enc: &mut wgpu::CommandEncoder) {
        if let Some(i) = self.cur {
            let q = i as u32 * 2;
            enc.write_timestamp(&self.set, q + 1);
            let at = i as u64 * wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT;
            enc.resolve_query_set(&self.set, q..q + 2, &self.resolve, at);
            enc.copy_buffer_to_buffer(&self.resolve, at, &self.reads[i].0, 0, 16);
        }
    }

    /// map the stamps after submit.
    pub fn after_submit(&mut self) {
        use std::sync::atomic::Ordering;
        if let Some(i) = self.cur.take() {
            let st = self.reads[i].1.clone();
            st.store(PENDING, Ordering::Release);
            self.reads[i].0.slice(..).map_async(wgpu::MapMode::Read, move |r| st.store(if r.is_ok() { MAPPED } else { FREE }, Ordering::Release));
        }
        self.next = (self.next + 1) % FRAMES;
    }

    /// read whatever came back, after a device poll.
    pub fn collect(&mut self) {
        use std::sync::atomic::Ordering;
        // oldest first so last_ms ends on the newest
        for k in 0..FRAMES {
            let i = (self.next + k) % FRAMES;
            let (buf, st) = &self.reads[i];
            if st.load(Ordering::Acquire) != MAPPED {
                continue;
            }
            if let Ok(data) = buf.slice(..).get_mapped_range() {
                let ts: &[u64] = bytemuck::cast_slice(&data[..16]);
                if ts[1] > ts[0] {
                    let ms = (ts[1] - ts[0]) as f64 * self.period / 1e6;
                    // drop nonsense pairs
                    if ms < 1000.0 {
                        self.last_ms = Some(ms);
                        self.frames += 1;
                    }
                }
            }
            buf.unmap();
            st.store(FREE, Ordering::Release);
        }
    }
}
