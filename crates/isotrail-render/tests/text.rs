use glam::IVec2;
use isotrail_render::{
    Aspect, Frame, OverlayStyle, OverlayText, Overlays, Palette, PanelStyle, PixelFont, SKY_BOTTOM,
    SKY_TOP, Stat, Subtitle, TextEffect, TitleSize,
};
use isotrail_testkit::assert_golden;

const POLISH: &str = "Przełęcz pod Łańcuchem | Zażółć gęślą jaźń";

fn filled(width: u32, height: u32, rgb: [u8; 3]) -> Frame {
    let [r, g, b] = rgb;
    Frame::new(
        width,
        height,
        [r, g, b, 255].repeat((width * height) as usize),
    )
}

#[test]
fn font_sample_matches_golden() {
    let font = PixelFont::monogram();
    let mut frame = filled(270, 104, [90, 120, 140]);
    let white = [255, 255, 255];
    let dark = [16, 20, 31];
    let lines = [
        (POLISH, 1, TextEffect::None),
        (
            "ŁÓDŹ, Kraków · Śnieżka 1603 m…",
            1,
            TextEffect::Shadow(dark),
        ),
        ("Zażółć | gęślą", 2, TextEffect::Shadow(dark)),
        ("Giewont | 1894 m", 2, TextEffect::Outline(dark)),
        ("ĄĆĘŃŚŹŻ ąćęńśźż !?#&@ 0123456789", 1, TextEffect::None),
    ];
    let mut y = 2;
    for (text, scale, effect) in lines {
        font.draw(&mut frame, text, IVec2::new(3, y), scale, white, effect);
        y += (PixelFont::LINE_HEIGHT * scale) as i32 + 2;
    }
    assert_golden("font-sample", frame.width(), frame.height(), frame.rgba());
}

#[test]
fn story_overlays_match_golden() {
    let framing = Aspect::Story.framing(3).unwrap();
    let mut frame = story_backdrop();
    let text = OverlayText {
        title: format!("{POLISH} - Hala Gąsienicowa"),
        subtitle: Subtitle::Auto,
        date: Some("Saturday 14 June 2025".into()),
        stats: vec![
            Stat::new("18.3 km", "distance"),
            Stat::new("1204 m", "gain"),
            Stat::new("6h 05m", "time"),
            Stat::new("2301 m", "high point"),
        ],
    };
    let overlays = Overlays::layout(&text, &OverlayStyle::default(), &framing);
    // The diorama keeps clear of the stats panel as well as the safe area
    assert!(overlays.diorama_framing().safe.bottom * 640.0 > 128.0);
    let palette = Palette::builtin("apollo").unwrap();
    overlays.draw(&mut frame, Some(&palette));
    assert_golden("overlay-story", frame.width(), frame.height(), frame.rgba());
}

/// The sky's gradient, with ground under the middle so the panels blur something
fn story_backdrop() -> Frame {
    let framing = Aspect::Story.framing(3).unwrap();
    let (width, height) = (framing.width, framing.height);
    let rgba = (0..height)
        .flat_map(|y| {
            let t = y as f32 / (height - 1) as f32;
            let [r, g, b] = std::array::from_fn(|c| {
                (f32::from(SKY_TOP[c]) + (f32::from(SKY_BOTTOM[c]) - f32::from(SKY_TOP[c])) * t)
                    as u8
            });
            [r, g, b, 255].repeat(width as usize)
        })
        .collect();
    let mut frame = Frame::new(width, height, rgba);
    for y in 300..560 {
        for x in 0..width as i32 {
            let shade = if (x / 7 + y / 5) % 2 == 0 { 70 } else { 120 };
            frame.set_pixel(x, y, [40, shade, 50, 255]);
        }
    }
    frame
}

#[test]
fn story_overlays_without_panels_match_golden() {
    let framing = Aspect::Story.framing(3).unwrap();
    let mut frame = story_backdrop();
    let text = OverlayText {
        title: POLISH.into(),
        subtitle: Subtitle::Text("Tatry Wysokie".into()),
        date: Some("14 Jun 2025".into()),
        stats: vec![
            Stat::new("18.3 km", "distance"),
            Stat::new("1188 m", "loss"),
            Stat::new("905 m", "low point"),
            Stat::new("3.0 km/h", "speed"),
            Stat::new("19:56 /km", "pace"),
            Stat::new("1d 2h", "elapsed"),
        ],
    };
    let style = OverlayStyle {
        panel: PanelStyle::None,
        title_size: TitleSize::Small,
    };
    let palette = Palette::builtin("apollo").unwrap();
    Overlays::layout(&text, &style, &framing).draw(&mut frame, Some(&palette));
    assert_golden(
        "overlay-story-plain",
        frame.width(),
        frame.height(),
        frame.rgba(),
    );
}
