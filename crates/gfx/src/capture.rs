//! Reading a rendered frame back off the GPU and writing it to a PNG.
//!
//! This exists so the app can screenshot *itself*. Capturing from outside — a
//! desktop screenshot utility — needs the display awake, the window focused and
//! frontmost, screen-recording permission, and the right crop rectangle guessed
//! from the outside. All four fail silently and produce a black or wrong image
//! rather than an error. Reading the swapchain texture back is none of those
//! things: it is exactly the pixels the GPU produced, available headless.
//!
//! It is also the machinery a pixel-level correctness test needs, which the
//! project does not yet have — right now wgpu tells us a frame was *accepted*,
//! never that it was *right*.

use std::path::Path;

/// Copying a texture to a buffer requires each row to start on a 256-byte
/// boundary, so a 1280px-wide BGRA row (5120 bytes) happens to fit exactly
/// while a 1281px one (5124) is padded to 5376.
///
/// Getting this wrong does not fail — it produces an image that shears
/// progressively further sideways with each row, which looks like a rendering
/// bug rather than a readback one.
fn padded_bytes_per_row(width: u32) -> u32 {
    let unpadded = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    unpadded.div_ceil(align) * align
}

/// A GPU buffer sized to receive one frame.
pub(crate) struct Readback {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
}

impl Readback {
    pub(crate) fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let size = u64::from(padded_bytes_per_row(width)) * u64::from(height);
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame readback"),
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            width,
            height,
        }
    }

    /// Records the copy. Must run before the frame is presented — the surface
    /// texture is gone afterwards.
    pub(crate) fn record(&self, encoder: &mut wgpu::CommandEncoder, frame: &wgpu::Texture) {
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: frame,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row(self.width)),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Maps the buffer and returns tightly-packed RGBA, padding removed.
    ///
    /// Blocks until the GPU is done, which is exactly what a screenshot — or a
    /// test that wants to look at the pixels — needs, and would be unacceptable
    /// inside a frame.
    ///
    /// `bgra` on the wire because that is the swapchain format. The surface is
    /// `Bgra8UnormSrgb`, so the bytes coming back are *already* sRGB-encoded and
    /// need no conversion; applying one here would double-encode and wash the
    /// image out.
    pub(crate) fn to_rgba(
        &self,
        device: &wgpu::Device,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let slice = self.buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;

        let padded = padded_bytes_per_row(self.width) as usize;
        let row = self.width as usize * 4;
        let mut rgba = Vec::with_capacity(row * self.height as usize);
        {
            let mapped = slice.get_mapped_range()?;
            for y in 0..self.height as usize {
                let (pixels, _) = mapped[y * padded..y * padded + row].as_chunks::<4>();
                for px in pixels {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                }
            }
        }
        self.buffer.unmap();
        Ok(rgba)
    }

    /// Writes the frame to a PNG.
    pub(crate) fn write_png(
        &self,
        device: &wgpu::Device,
        path: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let rgba = self.to_rgba(device)?;

        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = std::fs::File::create(path)?;
        let mut encoder =
            png::Encoder::new(std::io::BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&rgba)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule that shears an image sideways when it is wrong.
    #[test]
    fn rows_are_padded_to_the_copy_alignment() {
        assert_eq!(padded_bytes_per_row(1280), 5120, "already aligned");
        assert_eq!(padded_bytes_per_row(1281), 5376, "rounded up to 256");
        assert_eq!(padded_bytes_per_row(1), 256, "a single pixel still pads");

        for width in 1..2000u32 {
            let padded = padded_bytes_per_row(width);
            assert_eq!(padded % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0);
            assert!(padded >= width * 4);
        }
    }
}
