use glam::IVec2;

use crate::Gpu;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sprite {
    Start,
    Finish,
    Head,
}

/// Each sprite's image and the pixel that sits on the point it marks
const SOURCES: [(Sprite, &[u8], IVec2); 3] = [
    (
        Sprite::Start,
        include_bytes!("../../../assets/sprites/start.png"),
        IVec2::new(1, 11),
    ),
    (
        Sprite::Finish,
        include_bytes!("../../../assets/sprites/finish.png"),
        IVec2::new(1, 11),
    ),
    (
        Sprite::Head,
        include_bytes!("../../../assets/sprites/head.png"),
        IVec2::new(3, 3),
    ),
];

/// A sprite placed in the frame: its top-left pixel and size, and where it
/// sits in the atlas
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Placed {
    pub origin: IVec2,
    pub size: IVec2,
    pub atlas: IVec2,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    atlas_x: i32,
    size: IVec2,
    anchor: IVec2,
}

/// The sprites side by side in one texture
pub(crate) struct SpriteAtlas {
    pub view: wgpu::TextureView,
    entries: [Entry; SOURCES.len()],
}

impl SpriteAtlas {
    pub fn new(gpu: &Gpu) -> SpriteAtlas {
        let images = SOURCES.map(|(_, bytes, _)| decode(bytes));
        let width: u32 = images.iter().map(|(w, _, _)| w).sum();
        let height = images.iter().map(|(_, h, _)| *h).max().unwrap_or(1);
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        let mut x = 0;
        let entries = std::array::from_fn(|i| {
            let (w, h, pixels) = &images[i];
            for row in 0..*h {
                let from = (row * w * 4) as usize;
                let to = ((row * width + x) * 4) as usize;
                rgba[to..to + (w * 4) as usize]
                    .copy_from_slice(&pixels[from..from + (w * 4) as usize]);
            }
            let entry = Entry {
                atlas_x: x as i32,
                size: IVec2::new(*w as i32, *h as i32),
                anchor: SOURCES[i].2,
            };
            x += w;
            entry
        });
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("sprites"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        gpu.queue().write_texture(
            texture.as_image_copy(),
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );
        SpriteAtlas {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            entries,
        }
    }

    /// `sprite` with its anchor on `pixel`
    pub fn place(&self, sprite: Sprite, pixel: IVec2) -> Placed {
        let i = SOURCES
            .iter()
            .position(|(s, _, _)| *s == sprite)
            .expect("every sprite has a source");
        let entry = self.entries[i];
        Placed {
            origin: pixel - entry.anchor,
            size: entry.size,
            atlas: IVec2::new(entry.atlas_x, 0),
        }
    }
}

/// Width, height and RGBA8 pixels
pub(crate) fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::ALPHA | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().expect("shipped sprites decode");
    let mut rgba = vec![0; reader.output_buffer_size().expect("sprites are tiny")];
    let info = reader
        .next_frame(&mut rgba)
        .expect("shipped sprites decode");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    rgba.truncate(info.buffer_size());
    (info.width, info.height, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprites_decode_with_their_anchor_inside() {
        for (sprite, bytes, anchor) in SOURCES {
            let (w, h, rgba) = decode(bytes);
            assert_eq!(rgba.len(), (w * h * 4) as usize);
            assert!(anchor.x < w as i32 && anchor.y < h as i32, "{sprite:?}");
            let alpha = |x: i32, y: i32| rgba[((y as u32 * w + x as u32) * 4 + 3) as usize];
            assert_eq!(alpha(anchor.x, anchor.y), 255, "{sprite:?}");
            assert!(rgba.chunks(4).all(|p| p[3] == 0 || p[3] == 255));
        }
    }
}
