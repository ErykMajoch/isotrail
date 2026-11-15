use glam::Vec2;

use crate::{Background, Frame, Placement};

/// What shows behind a layer
#[derive(Clone, Copy, Debug)]
pub enum Fill<'a> {
    /// A frame of the picture's size before it is enlarged
    Frame(&'a Frame),
    Colour([u8; 3]),
    Clear,
}

impl<'a> Fill<'a> {
    /// What `background` puts behind a layer, with the sky drawn into `sky`
    pub fn behind(background: Background, sky: &'a Frame) -> Fill<'a> {
        match background {
            Background::Sky => Fill::Frame(sky),
            Background::Colour(rgb) => Fill::Colour(rgb),
            Background::Transparent => Fill::Clear,
        }
    }

    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        match *self {
            Fill::Frame(frame) if x < frame.width() && y < frame.height() => frame.pixel(x, y),
            Fill::Frame(_) | Fill::Clear => [0; 4],
            Fill::Colour([r, g, b]) => [r, g, b, 255],
        }
    }
}

/// How a layer's pixels are resized
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resample {
    /// Each pixel of the picture takes the nearest layer pixel, so every
    /// colour is one the layer or the fill already has
    Nearest,
    /// Layer pixels stay flat inside and blend across one pixel of the
    /// picture at their edges
    Sharp,
}

/// Lays `layer` over `fill` where `placement` puts it in a frame of `size`
/// pixels, each of them `scale` pixels of the picture across. A layer on
/// whole frame pixels at its own size is copied as it is
pub fn compose(
    fill: Fill,
    layer: &Frame,
    placement: &Placement,
    size: (u32, u32),
    scale: u32,
    resample: Resample,
) -> Frame {
    let scale = scale.max(1);
    let (width, height) = (size.0 * scale, size.1 * scale);
    let Placement {
        scale: grown,
        offset,
    } = *placement;
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    if grown == 1.0 && offset == offset.round() {
        let (left, top) = (offset.x as i64, offset.y as i64);
        for y in 0..height {
            for x in 0..width {
                let (fx, fy) = (x / scale, y / scale);
                let over = texel(layer, i64::from(fx) - left, i64::from(fy) - top);
                let pixel = if over[3] >= 128 {
                    over
                } else {
                    fill.at(fx, fy)
                };
                rgba.extend_from_slice(&pixel);
            }
        }
        return Frame::new(width, height, rgba);
    }

    // Pixels of the picture for each pixel of the layer
    let across = grown * scale as f32;
    for y in 0..height {
        for x in 0..width {
            let in_frame = (Vec2::new(x as f32, y as f32) + 0.5) / scale as f32;
            let in_layer = (in_frame - offset) / grown;
            let under = fill.at(x / scale, y / scale);
            let pixel = match resample {
                Resample::Nearest => {
                    let at = in_layer.floor();
                    let over = texel(layer, at.x as i64, at.y as i64);
                    if over[3] >= 128 { over } else { under }
                }
                Resample::Sharp => over_fill(blended(layer, in_layer, across), under),
            };
            rgba.extend_from_slice(&pixel);
        }
    }
    Frame::new(width, height, rgba)
}

/// See-through outside the layer
fn texel(layer: &Frame, x: i64, y: i64) -> [u8; 4] {
    if x < 0 || y < 0 || x >= i64::from(layer.width()) || y >= i64::from(layer.height()) {
        return [0; 4];
    }
    layer.pixel(x as u32, y as u32)
}

/// The layer's colour at `at`, as premultiplied shares of 255. The four
/// pixels round the point blend only within `1 / across` of an edge
fn blended(layer: &Frame, at: Vec2, across: f32) -> [f32; 4] {
    let corner = at - 0.5;
    let base = corner.floor();
    let weight = ((corner - base - 0.5) * across.max(1.0) + 0.5).clamp(Vec2::ZERO, Vec2::ONE);
    let load = |dx: i64, dy: i64| {
        let [r, g, b, a] = texel(layer, base.x as i64 + dx, base.y as i64 + dy).map(f32::from);
        let alpha = a / 255.0;
        [r * alpha, g * alpha, b * alpha, a]
    };
    let mix = |a: [f32; 4], b: [f32; 4], t: f32| {
        let mut out = [0.0; 4];
        for i in 0..4 {
            out[i] = a[i] + (b[i] - a[i]) * t;
        }
        out
    };
    mix(
        mix(load(0, 0), load(1, 0), weight.x),
        mix(load(0, 1), load(1, 1), weight.x),
        weight.y,
    )
}

fn over_fill(over: [f32; 4], under: [u8; 4]) -> [u8; 4] {
    let [r, g, b, a] = under.map(f32::from);
    let through = 1.0 - over[3] / 255.0;
    let under_alpha = a / 255.0 * through;
    let alpha = over[3] + a * through;
    if alpha <= 0.0 {
        return [0; 4];
    }
    let channel = |over: f32, under: f32| {
        ((over + under * under_alpha) * 255.0 / alpha)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    [
        channel(over[0], r),
        channel(over[1], g),
        channel(over[2], b),
        alpha.round().clamp(0.0, 255.0) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 4] = [200, 30, 30, 255];
    const GREEN: [u8; 4] = [30, 160, 60, 255];
    const BLUE: [u8; 4] = [20, 40, 220, 255];
    const SKY: [u8; 4] = [150, 190, 226, 255];
    const NONE: [u8; 4] = [0; 4];

    fn frame(width: u32, rows: &[&[[u8; 4]]]) -> Frame {
        let rgba = rows
            .iter()
            .flat_map(|row| row.iter().flatten().copied())
            .collect();
        Frame::new(width, rows.len() as u32, rgba)
    }

    fn sky(width: u32, height: u32) -> Frame {
        Frame::new(width, height, SKY.repeat((width * height) as usize))
    }

    fn colours(frame: &Frame) -> Vec<[u8; 4]> {
        let mut colours: Vec<[u8; 4]> = frame.rgba().as_chunks::<4>().0.to_vec();
        colours.sort_unstable();
        colours.dedup();
        colours
    }

    #[test]
    fn a_layer_on_whole_pixels_is_copied_as_it_is() {
        // A 2 x 1 frame inside a layer with one pixel of margin
        let layer = frame(
            4,
            &[
                &[BLUE, BLUE, BLUE, BLUE],
                &[BLUE, RED, NONE, BLUE],
                &[BLUE, BLUE, BLUE, BLUE],
            ],
        );
        let behind = sky(2, 1);
        let placement = Placement {
            scale: 1.0,
            offset: Vec2::new(-1.0, -1.0),
        };
        for resample in [Resample::Nearest, Resample::Sharp] {
            for scale in [1, 2, 3] {
                let picture = compose(
                    Fill::Frame(&behind),
                    &layer,
                    &placement,
                    (2, 1),
                    scale,
                    resample,
                );
                assert_eq!((picture.width(), picture.height()), (2 * scale, scale));
                for y in 0..scale {
                    for x in 0..2 * scale {
                        let expected = if x < scale { RED } else { SKY };
                        assert_eq!(picture.pixel(x, y), expected, "{x},{y} at {scale}x");
                    }
                }
            }
        }
    }

    #[test]
    fn fills_show_through_and_round_the_layer() {
        let layer = frame(2, &[&[RED, NONE]]);
        let placement = Placement {
            scale: 1.0,
            offset: Vec2::new(1.0, 0.0),
        };
        let size = (4, 1);
        let flat = compose(
            Fill::Colour([9, 8, 7]),
            &layer,
            &placement,
            size,
            1,
            Resample::Sharp,
        );
        assert_eq!(flat.pixel(0, 0), [9, 8, 7, 255]);
        assert_eq!(flat.pixel(1, 0), RED);
        assert_eq!(flat.pixel(2, 0), [9, 8, 7, 255]);
        assert_eq!(flat.pixel(3, 0), [9, 8, 7, 255]);
        let clear = compose(Fill::Clear, &layer, &placement, size, 1, Resample::Nearest);
        assert_eq!(clear.pixel(0, 0), NONE);
        assert_eq!(clear.pixel(1, 0), RED);
        assert_eq!(clear.pixel(2, 0), NONE);

        let behind = sky(4, 1);
        assert!(matches!(
            Fill::behind(Background::Sky, &behind),
            Fill::Frame(_)
        ));
        assert!(matches!(
            Fill::behind(Background::Colour([1, 2, 3]), &behind),
            Fill::Colour([1, 2, 3])
        ));
        assert!(matches!(
            Fill::behind(Background::Transparent, &behind),
            Fill::Clear
        ));
    }

    #[test]
    fn sharp_resizing_blends_only_at_edges() {
        let layer = frame(3, &[&[RED, GREEN, BLUE]]);
        // Half a pixel of the picture to the right, at 4 pixels to one
        let placement = Placement {
            scale: 1.0,
            offset: Vec2::new(0.125, 0.0),
        };
        let picture = compose(
            Fill::Colour([0, 0, 0]),
            &layer,
            &placement,
            (3, 1),
            4,
            Resample::Sharp,
        );
        let row: Vec<[u8; 4]> = (0..12).map(|x| picture.pixel(x, 0)).collect();
        assert_eq!(row[1..4], [RED; 3]);
        assert_eq!(row[5..8], [GREEN; 3]);
        assert_eq!(row[9..12], [BLUE; 3]);
        // The pixel an edge runs through the middle of is half of each
        assert_eq!(row[4], [115, 95, 45, 255]);
        assert_eq!(row[8], [25, 100, 140, 255]);
        // An edge onto nothing lets the fill through
        assert_eq!(row[0], [100, 15, 15, 255]);
    }

    #[test]
    fn nearest_resizing_makes_no_new_colour() {
        let layer = frame(
            4,
            &[
                &[RED, GREEN, BLUE, RED],
                &[GREEN, NONE, RED, BLUE],
                &[BLUE, RED, GREEN, NONE],
            ],
        );
        let behind = sky(3, 2);
        let allowed = [RED, GREEN, BLUE, SKY];
        for (grown, offset) in [(0.83, -0.4), (1.27, 0.31), (0.5, 0.0)] {
            let placement = Placement {
                scale: grown,
                offset: Vec2::new(offset, -offset),
            };
            for scale in [1, 3] {
                let picture = compose(
                    Fill::Frame(&behind),
                    &layer,
                    &placement,
                    (3, 2),
                    scale,
                    Resample::Nearest,
                );
                for colour in colours(&picture) {
                    assert!(allowed.contains(&colour), "{colour:?} at {grown}");
                }
            }
        }
    }

    #[test]
    fn a_resized_layer_keeps_its_place() {
        // A red pixel at layer (2, 2), shown twice the size from frame (1, 1)
        let mut rows = vec![vec![NONE; 5]; 5];
        rows[2][2] = RED;
        let rows: Vec<&[[u8; 4]]> = rows.iter().map(Vec::as_slice).collect();
        let layer = frame(5, &rows);
        let placement = Placement {
            scale: 2.0,
            offset: Vec2::new(-3.0, -3.0),
        };
        for resample in [Resample::Nearest, Resample::Sharp] {
            let picture = compose(Fill::Clear, &layer, &placement, (4, 4), 1, resample);
            for y in 0..4 {
                for x in 0..4 {
                    let inside = (1..3).contains(&x) && (1..3).contains(&y);
                    let expected = if inside { RED } else { NONE };
                    assert_eq!(picture.pixel(x, y), expected, "{x},{y} {resample:?}");
                }
            }
        }
    }
}
