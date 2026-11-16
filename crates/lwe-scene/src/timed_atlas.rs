//! CPU-only TEXS0003/BC3 region decoding. This API does not admit a wallpaper
//! into the renderer's compatibility subset or define its GPU sampling clock.
//!
//! A source seals the bytes read from one descriptor, not the underlying file
//! against concurrent in-place writes. Hashing and parsing use that same buffer.
//! The shared budget accounts for retained Vec capacities and reservations;
//! it is neither RSS nor allocator/driver memory measurement.

use std::mem::size_of;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::{
    AnimationFrame, MAX_ASSET_BYTES, MAX_TEXTURE_BYTES, MAX_TEXTURE_DIMENSION, MipMetadata,
    ResourceOrigin, Result, SceneAssetError, SceneResource, Texture, TextureInspectionBudget,
    TextureMetadata, limit,
};

/// Pixel coordinates in the original atlas, without implicit wrapping or padding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Validated region geometry. Fields are private so a decoder never accepts a
/// forged stride or an out-of-bounds block range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bc3RegionLayout {
    source_width: u32,
    source_height: u32,
    rect: PixelRect,
    source_bytes: usize,
    source_block_row_stride: usize,
    block_x: u32,
    block_y: u32,
    block_width: u32,
    block_height: u32,
    rgba_bytes: usize,
}

impl Bc3RegionLayout {
    pub fn checked(source_width: u32, source_height: u32, rect: PixelRect) -> Result<Self> {
        let source_bytes = bc3_size(source_width, source_height)?;
        if rect.width == 0 || rect.height == 0 {
            return Err(invalid("zero BC3 region dimension"));
        }
        let right = rect.x.checked_add(rect.width).ok_or_else(overflow)?;
        let bottom = rect.y.checked_add(rect.height).ok_or_else(overflow)?;
        if right > source_width || bottom > source_height {
            return Err(invalid("BC3 region outside source mip"));
        }
        let rgba_bytes = (rect.width as usize)
            .checked_mul(rect.height as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or_else(overflow)?;
        if rgba_bytes > MAX_TEXTURE_BYTES {
            return Err(limit(
                "BC3 region RGBA",
                rgba_bytes as u64,
                MAX_TEXTURE_BYTES as u64,
            ));
        }
        let block_x = rect.x / 4;
        let block_y = rect.y / 4;
        Ok(Self {
            source_width,
            source_height,
            rect,
            source_bytes,
            source_block_row_stride: source_width.div_ceil(4) as usize * 16,
            block_x,
            block_y,
            block_width: right.div_ceil(4) - block_x,
            block_height: bottom.div_ceil(4) - block_y,
            rgba_bytes,
        })
    }

    pub fn rect(&self) -> PixelRect {
        self.rect
    }
    pub fn source_bytes(&self) -> usize {
        self.source_bytes
    }
    pub fn source_block_row_stride(&self) -> usize {
        self.source_block_row_stride
    }
    pub fn block_origin(&self) -> [u32; 2] {
        [self.block_x, self.block_y]
    }
    pub fn block_dimensions(&self) -> [u32; 2] {
        [self.block_width, self.block_height]
    }
    pub fn rgba_bytes(&self) -> usize {
        self.rgba_bytes
    }

    /// Fixed block workspace; there are no packed/padded heap buffers or DDS
    /// library allocations on this path. This is a logical reservation, not a
    /// claim about the Rust call stack's total size.
    pub fn decoder_scratch_reservation(&self) -> usize {
        size_of::<Bc3Scratch>()
    }
    pub fn decode_reservation(&self) -> Result<usize> {
        self.rgba_bytes
            .checked_add(self.decoder_scratch_reservation())
            .ok_or_else(overflow)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasMemoryUsage {
    pub limit_bytes: usize,
    /// Live allocation capacity plus reservations held by in-flight operations.
    pub charged_bytes: usize,
    pub peak_charged_bytes: usize,
    pub active_decode_job: bool,
}

#[derive(Debug)]
struct MemoryState {
    used: usize,
    peak: usize,
    decoding: bool,
}

/// Share one budget across all source and region owners on a worker/engine.
/// Cloning an owner shares its actual allocation. Independently created sources
/// are charged independently, even when their encoded hash matches.
#[derive(Debug, Clone)]
pub struct AtlasMemoryBudget {
    limit: usize,
    state: Arc<Mutex<MemoryState>>,
}

impl AtlasMemoryBudget {
    pub fn new(limit_bytes: usize) -> Result<Self> {
        if limit_bytes == 0 || limit_bytes > MAX_ASSET_BYTES {
            return Err(limit(
                "timed atlas CPU budget",
                limit_bytes as u64,
                MAX_ASSET_BYTES as u64,
            ));
        }
        Ok(Self {
            limit: limit_bytes,
            state: Arc::new(Mutex::new(MemoryState {
                used: 0,
                peak: 0,
                decoding: false,
            })),
        })
    }

    pub fn usage(&self) -> AtlasMemoryUsage {
        // Only infallible integer updates occur under the lock; poisoning still
        // keeps the conservative counters accessible for cleanup and reporting.
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        AtlasMemoryUsage {
            limit_bytes: self.limit,
            charged_bytes: state.used,
            peak_charged_bytes: state.peak,
            active_decode_job: state.decoding,
        }
    }

    pub fn domain_id(&self) -> usize {
        Arc::as_ptr(&self.state) as usize
    }

    /// Charge an allocation owned by another CPU resource. The owner must keep
    /// this non-cloneable guard until after its buffer is released, and reconcile
    /// the actual Vec capacity before publishing the resource.
    pub fn reserve_bytes(&self, bytes: usize) -> Result<AtlasMemoryReservation> {
        self.reserve(bytes).map(AtlasMemoryReservation)
    }

    fn reserve(&self, bytes: usize) -> Result<MemoryLease> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let total = state.used.checked_add(bytes).ok_or_else(overflow)?;
        if total > self.limit {
            return Err(limit(
                "timed atlas CPU working set",
                total as u64,
                self.limit as u64,
            ));
        }
        state.used = total;
        state.peak = state.peak.max(total);
        Ok(MemoryLease {
            budget: self.clone(),
            bytes,
        })
    }

    fn begin_decode(&self) -> Result<DecodeJob> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.decoding {
            return Err(unsupported("concurrent timed atlas region decode"));
        }
        state.decoding = true;
        Ok(DecodeJob(self.clone()))
    }
}

#[derive(Debug)]
pub struct AtlasMemoryReservation(MemoryLease);

impl AtlasMemoryReservation {
    pub fn resize(&mut self, bytes: usize) -> Result<()> {
        self.0.resize(bytes)
    }
    pub fn bytes(&self) -> usize {
        self.0.bytes
    }
}

#[derive(Debug)]
struct MemoryLease {
    budget: AtlasMemoryBudget,
    bytes: usize,
}

impl MemoryLease {
    fn resize(&mut self, bytes: usize) -> Result<()> {
        let mut state = self.budget.state.lock().unwrap_or_else(|e| e.into_inner());
        let total = (state.used - self.bytes)
            .checked_add(bytes)
            .ok_or_else(overflow)?;
        if total > self.budget.limit {
            return Err(limit(
                "timed atlas CPU allocation capacity",
                total as u64,
                self.budget.limit as u64,
            ));
        }
        state.used = total;
        state.peak = state.peak.max(total);
        self.bytes = bytes;
        Ok(())
    }
}

impl Drop for MemoryLease {
    fn drop(&mut self) {
        let mut state = self.budget.state.lock().unwrap_or_else(|e| e.into_inner());
        state.used -= self.bytes;
    }
}

struct DecodeJob(AtlasMemoryBudget);
impl Drop for DecodeJob {
    fn drop(&mut self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .decoding = false;
    }
}

#[derive(Debug)]
struct OwnedBytes {
    // Drop the actual buffer before releasing its accounting lease.
    bytes: Vec<u8>,
    _lease: MemoryLease,
}

impl OwnedBytes {
    fn zeroed(length: usize, budget: &AtlasMemoryBudget) -> Result<Self> {
        let mut lease = budget.reserve(length)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| invalid("bounded atlas allocation failed"))?;
        lease.resize(bytes.capacity())?;
        bytes.resize(length, 0);
        Ok(Self {
            bytes,
            _lease: lease,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimedAtlasFrame {
    pub rect: PixelRect,
    /// Exact f32 duration promoted to f64, without an invented frame rate.
    pub duration_seconds: f64,
    pub end_seconds: f64,
}

#[derive(Debug)]
enum RawMip {
    EncodedRange { offset: usize, length: usize },
    Decompressed(OwnedBytes),
}

#[derive(Debug)]
struct AtlasSourceInner {
    encoded: OwnedBytes,
    raw: RawMip,
    metadata: TextureMetadata,
    frames: Vec<TimedAtlasFrame>,
    origin: ResourceOrigin,
    hash: [u8; 32],
    cycle: f64,
    budget: AtlasMemoryBudget,
    // Covers both metadata Vecs and the validated frame Vec, not scalar/Arc
    // overhead or allocator internals. Fields above drop before this lease.
    _framing_lease: MemoryLease,
}

#[derive(Debug, Clone)]
pub struct TimedAtlasSource(Arc<AtlasSourceInner>);

impl TimedAtlasSource {
    /// Read once from the supplied descriptor. The resulting immutable owners
    /// retain encoded framing and raw BC3, never a whole-atlas RGBA allocation.
    pub fn from_resource(
        resource: &SceneResource,
        inspection: &mut TextureInspectionBudget,
        budget: &AtlasMemoryBudget,
    ) -> Result<Self> {
        let length = usize::try_from(resource.len()).map_err(|_| overflow())?;
        if length > MAX_TEXTURE_BYTES {
            return Err(limit(
                "timed atlas encoded source",
                resource.len(),
                MAX_TEXTURE_BYTES as u64,
            ));
        }
        let mut lease = budget.reserve(length)?;
        let bytes = resource.read_range(0, length, MAX_TEXTURE_BYTES)?;
        lease.resize(bytes.capacity())?;
        Self::from_owned(
            OwnedBytes {
                bytes,
                _lease: lease,
            },
            resource.origin(),
            inspection,
            budget,
        )
    }

    /// Consume the same sealed bytes used for preparation identity. A physical
    /// owner and its non-cloneable reservation move together; no descriptor is
    /// reopened and no uncharged encoded copy is created.
    pub fn from_budgeted_bytes(
        bytes: Vec<u8>,
        reservation: AtlasMemoryReservation,
        origin: ResourceOrigin,
        inspection: &mut TextureInspectionBudget,
        budget: &AtlasMemoryBudget,
    ) -> Result<Self> {
        let mut encoded = OwnedBytes {
            bytes,
            _lease: reservation.0,
        };
        if encoded._lease.budget.domain_id() != budget.domain_id() {
            return Err(invalid(
                "timed source reservation belongs to another CPU budget",
            ));
        }
        if encoded.bytes.len() > MAX_TEXTURE_BYTES {
            return Err(limit(
                "timed atlas encoded source",
                encoded.bytes.len() as u64,
                MAX_TEXTURE_BYTES as u64,
            ));
        }
        if encoded._lease.bytes < encoded.bytes.capacity() {
            return Err(invalid("timed source capacity exceeds its reservation"));
        }
        encoded._lease.resize(encoded.bytes.capacity())?;
        Self::from_owned(encoded, origin, inspection, budget)
    }

    fn from_owned(
        encoded: OwnedBytes,
        origin: ResourceOrigin,
        inspection: &mut TextureInspectionBudget,
        budget: &AtlasMemoryBudget,
    ) -> Result<Self> {
        // Existing bounded framing parser's mip Vec can grow to the next power
        // of two; the maximum is already a power of two (1024 * 32). Frames
        // use exact declared capacity. Small reader scratch is reserved too.
        let mip_capacity = inspection
            .remaining_mips
            .clamp(1, 1024 * 32)
            .next_power_of_two()
            .max(4);
        let frame_capacity = inspection.remaining_animation_frames.min(4096);
        // Twice the final mip capacity also covers old+new Vec storage during
        // the existing parser's amortized growth, before the old buffer drops.
        let framing_reservation = mip_capacity
            .checked_mul(2 * size_of::<MipMetadata>())
            .and_then(|n| {
                n.checked_add(
                    frame_capacity * (size_of::<AnimationFrame>() + size_of::<TimedAtlasFrame>()),
                )
            })
            .and_then(|n| n.checked_add(1024))
            .ok_or_else(overflow)?;
        let mut framing_lease = budget.reserve(framing_reservation)?;
        let metadata = Texture::inspect_bounded(&encoded.bytes, inspection)?;
        let animation = metadata
            .animation
            .as_ref()
            .ok_or_else(|| unsupported("texture has no timed atlas"))?;
        if metadata.version != 5
            || metadata.body_version != 4
            || metadata.format != 4
            || metadata.flags != 7
            || metadata.container_format != -1
            || metadata.body_extra != Some(0)
            || metadata.images != 1
            || metadata.mips.len() != 1
            || metadata.video
            || animation.version != 3
        {
            return Err(unsupported(
                "timed atlas requires verified TEXB0004 single-mip BC3 flags7 / TEXS0003 framing",
            ));
        }
        // TEXI's opaque image-info word is retained and hashed, as in the
        // existing decoder; it is not a pixel stride/codec selector. Reading
        // this CPU subset does not infer that field's rendering semantics.
        let mip = &metadata.mips[0];
        if mip.image != 0
            || mip.level != 0
            || mip.width != metadata.storage_width
            || mip.height != metadata.storage_height
            || metadata.image_width != mip.width
            || metadata.image_height != mip.height
            || mip.video
        {
            return Err(unsupported("timed atlas mip/storage dimensions"));
        }
        let expected = bc3_size(mip.width, mip.height)?;
        // Check exact decompressed length before any LZ4 output allocation.
        if (mip.compressed && mip.decompressed_size as usize != expected)
            || (!mip.compressed
                && (mip.stored_size as usize != expected || mip.decompressed_size != 0))
        {
            return Err(invalid(
                "timed atlas BC3 byte count differs from source dimensions",
            ));
        }
        let mut frames = Vec::new();
        frames
            .try_reserve_exact(animation.frames.len())
            .map_err(|_| invalid("bounded frame allocation failed"))?;
        let actual_framing = metadata
            .mips
            .capacity()
            .checked_mul(size_of::<MipMetadata>())
            .and_then(|n| n.checked_add(animation.frames.capacity() * size_of::<AnimationFrame>()))
            .and_then(|n| n.checked_add(frames.capacity() * size_of::<TimedAtlasFrame>()))
            .ok_or_else(overflow)?;
        framing_lease.resize(actual_framing)?;
        let mut cycle = 0.0_f64;
        for frame in &animation.frames {
            let [x, y, width, shear_y, shear_x, height] = frame.coordinates;
            if frame.image != 0 || shear_y != 0.0 || shear_x != 0.0 {
                return Err(unsupported(
                    "timed atlas nonrectangular or multi-image frame",
                ));
            }
            let rect = PixelRect {
                x: integral(x)?,
                y: integral(y)?,
                width: integral(width)?,
                height: integral(height)?,
            };
            Bc3RegionLayout::checked(mip.width, mip.height, rect)?;
            if rect.width != animation.width || rect.height != animation.height {
                return Err(unsupported(
                    "timed atlas frame extent differs from TEXS0003 extent",
                ));
            }
            let duration = frame.duration as f64;
            let end = cycle + duration;
            if !duration.is_finite() || duration <= 0.0 || !end.is_finite() || end <= cycle {
                return Err(invalid(
                    "timed atlas duration cannot form a finite strictly increasing timeline",
                ));
            }
            frames.push(TimedAtlasFrame {
                rect,
                duration_seconds: duration,
                end_seconds: end,
            });
            cycle = end;
        }
        let stored_end = mip
            .offset
            .checked_add(mip.stored_size as usize)
            .ok_or_else(overflow)?;
        let stored = encoded
            .bytes
            .get(mip.offset..stored_end)
            .ok_or_else(|| invalid("timed atlas mip outside sealed source"))?;
        let raw = if mip.compressed {
            let mut output = OwnedBytes::zeroed(expected, budget)?;
            let decoded = lz4_flex::block::decompress_into(stored, &mut output.bytes)
                .map_err(|e| invalid(format!("invalid timed atlas LZ4 mip: {e}")))?;
            if decoded != expected {
                return Err(invalid("timed atlas LZ4 size differs from exact BC3 size"));
            }
            RawMip::Decompressed(output)
        } else {
            RawMip::EncodedRange {
                offset: mip.offset,
                length: expected,
            }
        };
        let hash = Sha256::digest(&encoded.bytes).into();
        Ok(Self(Arc::new(AtlasSourceInner {
            encoded,
            raw,
            metadata,
            frames,
            origin,
            hash,
            cycle,
            budget: budget.clone(),
            _framing_lease: framing_lease,
        })))
    }

    pub fn encoded_bytes(&self) -> &[u8] {
        &self.0.encoded.bytes
    }
    pub fn encoded_sha256(&self) -> [u8; 32] {
        self.0.hash
    }
    pub fn origin(&self) -> ResourceOrigin {
        self.0.origin
    }
    pub fn metadata(&self) -> &TextureMetadata {
        &self.0.metadata
    }
    pub fn frames(&self) -> &[TimedAtlasFrame] {
        &self.0.frames
    }
    pub fn cycle_seconds(&self) -> f64 {
        self.0.cycle
    }
    pub fn encoded_capacity(&self) -> usize {
        self.0.encoded.bytes.capacity()
    }
    pub fn raw_capacity(&self) -> usize {
        match &self.0.raw {
            RawMip::EncodedRange { .. } => 0,
            RawMip::Decompressed(bytes) => bytes.bytes.capacity(),
        }
    }
    pub fn retained_capacity_bytes(&self) -> usize {
        self.encoded_capacity() + self.raw_capacity() + self.0._framing_lease.bytes
    }

    pub fn allocation_id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }

    pub fn memory_budget(&self) -> &AtlasMemoryBudget {
        &self.0.budget
    }

    /// Pure half-open [begin,end) timeline selection. The caller owns startup,
    /// pause/resume and transaction clock semantics; this API invents none.
    pub fn frame_at(&self, elapsed_seconds: f64) -> Result<usize> {
        if !elapsed_seconds.is_finite() || elapsed_seconds < 0.0 {
            return Err(invalid(
                "timed atlas elapsed time must be finite and nonnegative",
            ));
        }
        let phase = elapsed_seconds % self.0.cycle;
        let index = self
            .0
            .frames
            .partition_point(|frame| frame.end_seconds <= phase);
        if index == self.0.frames.len() {
            return Err(invalid("timed atlas phase outside finite timeline"));
        }
        Ok(index)
    }

    pub fn frame_layout(&self, index: usize) -> Result<Bc3RegionLayout> {
        let frame = self
            .0
            .frames
            .get(index)
            .ok_or_else(|| invalid("timed atlas frame index outside table"))?;
        self.region_layout(frame.rect)
    }

    pub fn region_layout(&self, rect: PixelRect) -> Result<Bc3RegionLayout> {
        Bc3RegionLayout::checked(
            self.0.metadata.storage_width,
            self.0.metadata.storage_height,
            rect,
        )
    }

    /// Expand to real neighboring atlas pixels and clip only at the outer atlas
    /// bounds. No edge replication, transparent border, clamp/repeat or GPU UV
    /// convention is implied; corner regions may have different dimensions.
    pub fn frame_neighborhood(&self, index: usize, radius: u32) -> Result<Bc3RegionLayout> {
        let rect = self.frame_layout(index)?.rect();
        let right = rect
            .x
            .checked_add(rect.width)
            .and_then(|n| n.checked_add(radius))
            .ok_or_else(overflow)?
            .min(self.0.metadata.storage_width);
        let bottom = rect
            .y
            .checked_add(rect.height)
            .and_then(|n| n.checked_add(radius))
            .ok_or_else(overflow)?
            .min(self.0.metadata.storage_height);
        let x = rect.x.saturating_sub(radius);
        let y = rect.y.saturating_sub(radius);
        self.region_layout(PixelRect {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }

    pub fn decode_frame(&self, index: usize) -> Result<TimedAtlasRegion> {
        self.decode_region(self.frame_layout(index)?.rect())
    }

    pub fn decode_region(&self, rect: PixelRect) -> Result<TimedAtlasRegion> {
        let layout = self.region_layout(rect)?;
        let _job = self.0.budget.begin_decode()?;
        // Reserve both buffers before allocation; scratch reservation is fixed.
        let _scratch_lease = self
            .0
            .budget
            .reserve(layout.decoder_scratch_reservation())?;
        let mut rgba = OwnedBytes::zeroed(layout.rgba_bytes, &self.0.budget)?;
        let raw = match &self.0.raw {
            RawMip::EncodedRange { offset, length } => {
                &self.0.encoded.bytes[*offset..*offset + *length]
            }
            RawMip::Decompressed(bytes) => &bytes.bytes,
        };
        decode_region_into(raw, &layout, &mut rgba.bytes)?;
        Ok(TimedAtlasRegion(Arc::new(RegionInner { rgba, layout })))
    }
}

#[derive(Debug)]
struct RegionInner {
    rgba: OwnedBytes,
    layout: Bc3RegionLayout,
}

/// An immutable, budget-owning region. Clones share the allocation; there is no
/// Vec extraction API that could leave pixels alive after releasing the lease.
#[derive(Debug, Clone)]
pub struct TimedAtlasRegion(Arc<RegionInner>);
impl TimedAtlasRegion {
    pub fn rgba(&self) -> &[u8] {
        &self.0.rgba.bytes
    }
    pub fn capacity_bytes(&self) -> usize {
        self.0.rgba.bytes.capacity()
    }
    pub fn layout(&self) -> Bc3RegionLayout {
        self.0.layout
    }

    pub fn allocation_id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}

fn bc3_size(width: u32, height: u32) -> Result<usize> {
    if width == 0 || height == 0 {
        return Err(invalid("zero BC3 source dimension"));
    }
    if width > MAX_TEXTURE_DIMENSION || height > MAX_TEXTURE_DIMENSION {
        return Err(limit(
            "BC3 source dimension",
            width.max(height) as u64,
            MAX_TEXTURE_DIMENSION as u64,
        ));
    }
    // Retain the existing per-texture decoded extent ceiling even though this
    // decoder never allocates the complete extent as RGBA.
    let full_rgba = u64::from(width) * u64::from(height) * 4;
    if full_rgba > MAX_TEXTURE_BYTES as u64 {
        return Err(limit(
            "BC3 source RGBA extent",
            full_rgba,
            MAX_TEXTURE_BYTES as u64,
        ));
    }
    (width.div_ceil(4) as usize)
        .checked_mul(height.div_ceil(4) as usize)
        .and_then(|n| n.checked_mul(16))
        .ok_or_else(overflow)
}

fn integral(value: f32) -> Result<u32> {
    if !value.is_finite()
        || value < 0.0
        || value.fract() != 0.0
        || value > MAX_TEXTURE_DIMENSION as f32
    {
        return Err(unsupported(
            "timed atlas fractional, negative or oversized frame coordinate",
        ));
    }
    Ok(value as u32)
}

struct Bc3Scratch {
    pixels: [u8; 64],
    colors: [[u8; 3]; 4],
    alphas: [u8; 8],
}

impl Default for Bc3Scratch {
    fn default() -> Self {
        Self {
            pixels: [0; 64],
            colors: [[0; 3]; 4],
            alphas: [0; 8],
        }
    }
}

fn decode_region_into(raw: &[u8], layout: &Bc3RegionLayout, rgba: &mut [u8]) -> Result<()> {
    if raw.len() != layout.source_bytes || rgba.len() != layout.rgba_bytes {
        return Err(invalid(
            "BC3 source/output byte count differs from validated layout",
        ));
    }
    let mut scratch = Bc3Scratch::default();
    let rect = layout.rect;
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    let output_stride = rect.width as usize * 4;
    let source_row_bytes = layout.block_width as usize * 16;
    for by in layout.block_y..layout.block_y + layout.block_height {
        let top = by * 4;
        let y0 = top.max(rect.y);
        let rows = ((top + 4).min(bottom) - y0) as usize;
        let source_pixel_row = (y0 - top) as usize * 16;
        let output_row = (y0 - rect.y) as usize * output_stride;
        let source_row =
            by as usize * layout.source_block_row_stride + layout.block_x as usize * 16;
        // The checked layout selects complete BC3 blocks, even at partial mip
        // edges. Only pixel copies need clipping; the source row remains whole.
        for (index, block) in raw[source_row..source_row + source_row_bytes]
            .as_chunks::<16>()
            .0
            .iter()
            .enumerate()
        {
            decode_block(block, &mut scratch);
            let left = (layout.block_x + index as u32) * 4;
            if rows == 4 && left >= rect.x && left + 4 <= right {
                let output = output_row + (left - rect.x) as usize * 4;
                // Internal blocks always copy four complete, fixed-size rows.
                for row in 0..4 {
                    let from = row * 16;
                    let to = output + row * output_stride;
                    rgba[to..to + 16].copy_from_slice(&scratch.pixels[from..from + 16]);
                }
            } else {
                let x0 = left.max(rect.x);
                let length = ((left + 4).min(right) - x0) as usize * 4;
                let source = source_pixel_row + (x0 - left) as usize * 4;
                let output = output_row + (x0 - rect.x) as usize * 4;
                for row in 0..rows {
                    let from = source + row * 16;
                    let to = output + row * output_stride;
                    rgba[to..to + length].copy_from_slice(&scratch.pixels[from..from + length]);
                }
            }
        }
    }
    Ok(())
}

fn decode_block(block: &[u8], scratch: &mut Bc3Scratch) {
    // Independently expressed BC3 equations from EXT_texture_compression_s3tc:
    // https://registry.khronos.org/OpenGL/extensions/EXT/EXT_texture_compression_s3tc.txt
    // Integer expansion/interpolation matches the retained image DDS decoder;
    // CPU oracle agreement is not a claim about hardware sampling/rounding.
    for (index, offset) in [(0, 8), (1, 10)] {
        let rgb = u16::from_le_bytes([block[offset], block[offset + 1]]);
        scratch.colors[index] = [
            (((rgb >> 11) & 31) * 255 / 31) as u8,
            (((rgb >> 5) & 63) * 255 / 63) as u8,
            ((rgb & 31) * 255 / 31) as u8,
        ];
    }
    // BC3 always uses all four colors, even when endpoint0 <= endpoint1.
    for channel in 0..3 {
        let a = u16::from(scratch.colors[0][channel]);
        let b = u16::from(scratch.colors[1][channel]);
        scratch.colors[2][channel] = ((2 * a + b + 1) / 3) as u8;
        scratch.colors[3][channel] = ((a + 2 * b + 1) / 3) as u8;
    }
    let a = u16::from(block[0]);
    let b = u16::from(block[1]);
    scratch.alphas[0] = a as u8;
    scratch.alphas[1] = b as u8;
    if a > b {
        for (i, alpha) in scratch.alphas.iter_mut().enumerate().skip(2) {
            *alpha = (((8 - i) as u16 * a + (i - 1) as u16 * b) / 7) as u8;
        }
    } else {
        for (i, alpha) in scratch.alphas[..6].iter_mut().enumerate().skip(2) {
            *alpha = (((6 - i) as u16 * a + (i - 1) as u16 * b) / 5) as u8;
        }
        scratch.alphas[6] = 0;
        scratch.alphas[7] = 255;
    }
    let colors = u32::from_le_bytes(block[12..16].try_into().expect("validated BC3 block"));
    let alpha = block[2..8]
        .iter()
        .enumerate()
        .fold(0_u64, |bits, (i, byte)| {
            bits | (u64::from(*byte) << (i * 8))
        });
    for pixel in 0..16 {
        let color = ((colors >> (2 * pixel)) & 3) as usize;
        let a = ((alpha >> (3 * pixel)) & 7) as usize;
        scratch.pixels[pixel * 4..pixel * 4 + 3].copy_from_slice(&scratch.colors[color]);
        scratch.pixels[pixel * 4 + 3] = scratch.alphas[a];
    }
}

fn invalid(reason: impl Into<String>) -> SceneAssetError {
    SceneAssetError::InvalidTexture {
        reason: reason.into(),
    }
}
fn unsupported(feature: impl Into<String>) -> SceneAssetError {
    SceneAssetError::Unsupported {
        feature: feature.into(),
    }
}
fn overflow() -> SceneAssetError {
    invalid("timed atlas size arithmetic overflow")
}

#[cfg(test)]
mod tests;
