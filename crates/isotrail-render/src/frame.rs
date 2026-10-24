/// An RGBA8 image in CPU memory, rows top to bottom with no padding
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Frame {
    /// # Panics
    ///
    /// If `rgba` doesn't hold exactly `width × height` pixels
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        assert_eq!(
            rgba.len(),
            width as usize * height as usize * 4,
            "RGBA buffer doesn't match a {width} × {height} frame"
        );
        Self {
            width,
            height,
            rgba,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub fn into_rgba(self) -> Vec<u8> {
        self.rgba
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}
