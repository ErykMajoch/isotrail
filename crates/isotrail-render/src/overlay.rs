use std::collections::HashMap;

use glam::IVec2;
use isotrail_bundle::{HikeStats, SceneBundle, Units};

use crate::{Frame, Framing, Insets, Palette, PixelFont, RouteStyle, TextEffect};

/// Text colour, and the softer colour of subtitles, dates and labels
pub const TEXT: [u8; 3] = [235, 237, 233];
pub const SUBTEXT: [u8; 3] = [164, 221, 219];
pub const TEXT_SHADOW: [u8; 3] = [16, 20, 31];
/// Panels blur what lies under them and blend it with this colour
pub const PANEL_TINT: [u8; 3] = [32, 46, 55];
const PANEL_TINT_SHARE: f32 = 0.5;
const PANEL_BLUR_PX: i32 = 3;
/// The panel's top edge catches the light
const PANEL_EDGE: [u8; 3] = [129, 151, 150];

/// Title separators, earliest first, where a title too long for two large
/// lines splits into a title and a subtitle
const SEPARATORS: [&str; 4] = [" | ", " - ", " – ", " — "];
const TITLE_LINES: usize = 2;
const SUBTITLE_LINES: usize = 2;
/// Rows of the glyph cell a line box covers: lowercase accents to descenders
const BOX_TOP: u32 = 2;
const BOX_ROWS: u32 = PixelFont::HEIGHT - BOX_TOP;
/// The row under the capitals, where large figures and small units line up
const BASELINE: u32 = PixelFont::CAP_TOP + 7;
/// Height of the profile strip, in large text pixels
const STRIP_ROWS: u32 = 12;
/// The most of a landscape frame's width its side column takes
const COLUMN_SHARE: f32 = 0.4;
/// The profile ahead of the walked part, which takes the route's colour
const PROFILE_AHEAD: [u8; 3] = [168, 181, 178];

/// The words drawn over a render
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayText {
    /// Empty for no title block
    pub title: String,
    pub subtitle: Subtitle,
    /// Local start date, ready to print
    pub date: Option<String>,
    pub stats: Vec<Stat>,
    /// Elevations evenly spaced along the route; fewer than two draw no profile
    pub profile: Vec<f32>,
    /// Credit for the map and elevation data
    pub attribution: Option<String>,
}

/// What goes under the title
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Subtitle {
    /// The rest of a title too long for its lines, from its first separator
    #[default]
    Auto,
    Text(String),
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stat {
    pub value: String,
    pub label: String,
}

/// The statistics an overlay can show
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatKind {
    Distance,
    Gain,
    Loss,
    /// Moving time, or elapsed time if the track has none
    Time,
    Elapsed,
    High,
    Low,
    /// Average over the moving time
    Speed,
    Pace,
}

pub const DEFAULT_STATS: [StatKind; 4] = [
    StatKind::Distance,
    StatKind::Gain,
    StatKind::Time,
    StatKind::High,
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DateFormat {
    /// `Saturday 14 June 2025`
    #[default]
    Long,
    /// `14 Jun 2025`
    Short,
    /// `2025-06-14`
    Iso,
}

impl OverlayText {
    /// The bundle's title, start date, the chosen stats, leaving out those
    /// the track didn't record, its profile and its data credits
    pub fn from_bundle(
        bundle: &SceneBundle,
        units: Units,
        stats: &[StatKind],
        date_format: DateFormat,
    ) -> OverlayText {
        let meta = &bundle.meta;
        OverlayText {
            title: meta.title.clone(),
            subtitle: Subtitle::Auto,
            date: meta
                .local_date
                .as_deref()
                .and_then(|date| format_date(date, date_format)),
            stats: stats
                .iter()
                .filter_map(|kind| kind.stat(&meta.stats, units))
                .collect(),
            profile: bundle.profile.iter().map(|sample| sample.ele_m).collect(),
            attribution: (!meta.attribution.is_empty()).then(|| meta.attribution.join(" · ")),
        }
    }
}

impl Stat {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Stat {
        Stat {
            value: value.into(),
            label: label.into(),
        }
    }
}

impl StatKind {
    /// `None` if the track didn't record what this needs
    pub fn stat(self, stats: &HikeStats, units: Units) -> Option<Stat> {
        let elevation = |m: f64| format!("{:.0} {}", units.elevation(m), units.elevation_unit());
        let time = stats.moving_s.or(stats.elapsed_s).filter(|&s| s > 0);
        let distance = units.distance(stats.distance_m);
        let (value, label) = match self {
            StatKind::Distance => (format_distance(stats.distance_m, units), "distance"),
            StatKind::Gain => (elevation(stats.gain_m?), "gain"),
            StatKind::Loss => (elevation(stats.loss_m?), "loss"),
            StatKind::Time => (format_duration(time?), "time"),
            StatKind::Elapsed => (format_elapsed(stats.elapsed_s?), "elapsed"),
            StatKind::High => (elevation(stats.max_ele_m?), "high point"),
            StatKind::Low => (elevation(stats.min_ele_m?), "low point"),
            StatKind::Speed => {
                let unit = match units {
                    Units::Metric => "km/h",
                    Units::Imperial => "mph",
                };
                let speed = distance / (time? as f64 / 3600.0);
                (format!("{speed:.1} {unit}"), "speed")
            }
            StatKind::Pace => {
                if distance <= 0.0 {
                    return None;
                }
                let seconds = (time? as f64 / distance).round() as i64;
                let pace = format!("{}:{:02}", seconds / 60, seconds % 60);
                (format!("{pace} /{}", units.distance_unit()), "pace")
            }
        };
        Some(Stat::new(value, label))
    }
}

/// One decimal below 100, whole units above
fn format_distance(metres: f64, units: Units) -> String {
    let value = units.distance(metres);
    let unit = units.distance_unit();
    if value < 99.95 {
        format!("{value:.1} {unit}")
    } else {
        format!("{value:.0} {unit}")
    }
}

fn format_duration(seconds: i64) -> String {
    let minutes = (seconds.max(0) + 30) / 60;
    if minutes < 60 {
        format!("{minutes}m")
    } else {
        format!("{}h {:02}m", minutes / 60, minutes % 60)
    }
}

/// Days and hours once it reaches a day, as multi-day hikes span nights
fn format_elapsed(seconds: i64) -> String {
    if seconds < 86_400 {
        return format_duration(seconds);
    }
    let hours = (seconds + 1800) / 3600;
    format!("{}d {}h", hours / 24, hours % 24)
}

/// A `YYYY-MM-DD` date in `format`, or `None` if it isn't a real date
pub fn format_date(iso: &str, format: DateFormat) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    const WEEKDAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let mut parts = iso.splitn(3, '-').map(str::parse::<u32>);
    let (year, month, day) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1..=12 => 31,
        _ => return None,
    };
    if day == 0 || day > days_in_month {
        return None;
    }
    let month_name = MONTHS[month as usize - 1];
    Some(match format {
        DateFormat::Long => {
            // Sakamoto's method
            const OFFSETS: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
            let y = if month < 3 { year - 1 } else { year };
            let weekday = (y + y / 4 - y / 100 + y / 400 + OFFSETS[month as usize - 1] + day) % 7;
            format!("{} {day} {month_name} {year}", WEEKDAYS[weekday as usize])
        }
        DateFormat::Short => format!("{day} {} {year}", &month_name[..3]),
        DateFormat::Iso => format!("{year:04}-{month:02}-{day:02}"),
    })
}

/// Whether text sits on frosted panels or straight on the scene
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelStyle {
    #[default]
    Frosted,
    /// Text keeps a dark outline instead
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TitleSize {
    #[default]
    Large,
    /// The size of the subtitle and date
    Small,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverlayStyle {
    pub panel: PanelStyle,
    pub title_size: TitleSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Text,
    Subtext,
    /// Outlined, as it sits straight on the scene
    Credit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    text: String,
    /// Top-left of the glyph cell
    origin: IVec2,
    scale: u32,
    tone: Tone,
}

/// Inclusive-exclusive pixel bounds
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    min: IVec2,
    max: IVec2,
}

/// Lines laid out around x = 0 from y = 0 down, and the space they take
#[derive(Clone, Debug, Default)]
struct Block {
    lines: Vec<Line>,
    width: i32,
    height: i32,
}

#[derive(Clone, Copy, Debug)]
struct Metrics {
    large: u32,
    small: u32,
    /// Between a panel's edge and what it holds
    pad: IVec2,
}

impl Metrics {
    /// Between the parts of a panel
    fn gap(&self) -> i32 {
        2 * self.pad.y
    }

    fn strip_height(&self) -> i32 {
        (STRIP_ROWS * self.large) as i32
    }
}

/// Where the panels, the profile strip and the lines sit in a frame
#[derive(Clone, Debug, PartialEq)]
pub struct Overlays {
    framing: Framing,
    style: OverlayStyle,
    panels: Vec<Rect>,
    lines: Vec<Line>,
    strip: Option<Rect>,
    profile: Vec<f32>,
    /// Pixels the overlays take, counted from the frame's edges
    top_px: u32,
    bottom_px: u32,
    left_px: u32,
}

impl Overlays {
    /// Portrait and square frames hang the title block from the top of the
    /// safe area and stand the stats block, with the profile and the credit
    /// under it, on its bottom; landscape frames stack everything in one
    /// column on the left. Large text is two pixels per glyph pixel in a
    /// 360-pixel-wide frame, and keeps that size on screen at other pixel
    /// scales
    pub fn layout(text: &OverlayText, style: &OverlayStyle, framing: &Framing) -> Overlays {
        let (width, height) = (framing.width, framing.height);
        let large = (width.min(height) / 180).clamp(1, 3);
        let metrics = Metrics {
            large,
            small: (large / 2).max(1),
            pad: IVec2::new(3 * large as i32, 2 * large as i32),
        };
        // Half the diorama's side margin: the sides of a story stay uncovered
        let margin = ((width as f32 * framing.safe.left / 2.0).round() as i32).max(2);
        let mut layout = Overlays {
            framing: *framing,
            style: *style,
            panels: Vec::new(),
            lines: Vec::new(),
            strip: None,
            profile: if text.profile.len() >= 2 {
                text.profile.clone()
            } else {
                Vec::new()
            },
            top_px: 0,
            bottom_px: 0,
            left_px: 0,
        };
        if width > height {
            let inner = (width as f32 * COLUMN_SHARE).round() as i32 - 2 * metrics.pad.x;
            layout.place_column(text, &metrics, margin, inner.max(1));
        } else {
            let max_width = width as i32 - 2 * margin - 2 * metrics.pad.x;
            layout.place_stacked(text, &metrics, max_width.max(1));
        }
        layout
    }

    /// The framing with its safe area grown to keep the diorama clear of
    /// the overlays
    pub fn diorama_framing(&self) -> Framing {
        let gap = 4.0 * (self.framing.width.min(self.framing.height) / 180).max(1) as f32;
        let share = |px: u32, size: u32| {
            if px == 0 {
                0.0
            } else {
                (px as f32 + gap) / size as f32
            }
        };
        let (width, height) = (self.framing.width, self.framing.height);
        let safe = self.framing.safe;
        Framing {
            safe: Insets {
                top: safe.top.max(share(self.top_px, height)),
                bottom: safe.bottom.max(share(self.bottom_px, height)),
                left: safe.left.max(share(self.left_px, width)),
                ..safe
            },
            ..self.framing
        }
    }

    /// Draws the panels, if the style has them, the profile with its first
    /// `progress` share walked, and the text, with every colour moved to the
    /// nearest in `palette`
    pub fn draw(&self, frame: &mut Frame, palette: Option<&Palette>, progress: f32) {
        let mut snap = Snap::new(palette);
        let shadow = snap.colour(TEXT_SHADOW);
        let outline = TextEffect::Outline(shadow);
        let effect = match self.style.panel {
            PanelStyle::Frosted => {
                for panel in &self.panels {
                    draw_panel(frame, *panel, &mut snap);
                }
                TextEffect::Shadow(shadow)
            }
            PanelStyle::None => outline,
        };
        if let Some(strip) = self.strip {
            draw_profile(frame, strip, &self.profile, progress, &mut snap);
        }
        let font = PixelFont::monogram();
        for line in &self.lines {
            let (colour, effect) = match line.tone {
                Tone::Text => (TEXT, effect),
                Tone::Subtext => (SUBTEXT, effect),
                Tone::Credit => (SUBTEXT, outline),
            };
            let colour = snap.colour(colour);
            font.draw(frame, &line.text, line.origin, line.scale, colour, effect);
        }
    }

    fn place_stacked(&mut self, text: &OverlayText, metrics: &Metrics, max_width: i32) {
        let (width, height) = (self.framing.width as i32, self.framing.height as i32);
        let safe = self.framing.safe;
        let pad = metrics.pad;
        let centre = width / 2;

        let heading = heading(text, &self.style, metrics, max_width as u32);
        if !heading.lines.is_empty() {
            let top = (height as f32 * safe.top).round() as i32;
            let bottom = top + heading.height + 2 * pad.y;
            self.panels
                .push(panel_rect(centre, heading.width, top, bottom, pad));
            self.put(heading, IVec2::new(centre, top + pad.y));
            self.top_px = bottom.max(0) as u32;
        }

        let safe_bottom = (height as f32 * (1.0 - safe.bottom)).round() as i32;
        let mut reached = safe_bottom;
        let mut bottom = safe_bottom;
        let credit = credit(text, metrics, max_width as u32);
        if !credit.lines.is_empty() {
            reached = bottom - credit.height;
            self.put(credit, IVec2::new(centre, reached));
            bottom = reached - pad.y;
        }
        let stats = stats_block(&text.stats, metrics, max_width);
        let has_stats = !stats.lines.is_empty();
        let has_strip = !self.profile.is_empty();
        if has_stats || has_strip {
            let inner_width = if has_stats {
                stats.width
            } else {
                max_width * 3 / 4
            };
            let mut inner_height = stats.height;
            if has_strip {
                if has_stats {
                    inner_height += metrics.gap();
                }
                inner_height += metrics.strip_height();
                let half = (inner_width + 1) / 2;
                self.strip = Some(Rect {
                    min: IVec2::new(centre - half, bottom - pad.y - metrics.strip_height()),
                    max: IVec2::new(centre + half, bottom - pad.y),
                });
            }
            reached = bottom - inner_height - 2 * pad.y;
            self.panels
                .push(panel_rect(centre, inner_width, reached, bottom, pad));
            self.put(stats, IVec2::new(centre, reached + pad.y));
        }
        if reached < safe_bottom {
            self.bottom_px = (height - reached).max(0) as u32;
        }
    }

    /// One panel from the left margin holding the title, stats and profile,
    /// centred in the safe area's height with the credit under it
    fn place_column(&mut self, text: &OverlayText, metrics: &Metrics, margin: i32, inner: i32) {
        let height = self.framing.height as i32;
        let safe = self.framing.safe;
        let pad = metrics.pad;
        let outer = inner + 2 * pad.x;
        let centre = margin + outer / 2;

        let heading = heading(text, &self.style, metrics, inner as u32);
        let stats = stats_block(&text.stats, metrics, inner);
        let credit = credit(text, metrics, outer as u32);
        let strip = (!self.profile.is_empty()).then(|| Block {
            height: metrics.strip_height(),
            ..Block::default()
        });
        let parts: Vec<Block> = [Some(heading), Some(stats), strip]
            .into_iter()
            .flatten()
            .filter(|part| part.height > 0)
            .collect();
        let panel_height = if parts.is_empty() {
            0
        } else {
            parts.iter().map(|part| part.height).sum::<i32>()
                + (parts.len() as i32 - 1) * metrics.gap()
                + 2 * pad.y
        };
        let credit_height = match (credit.lines.is_empty(), parts.is_empty()) {
            (true, _) => 0,
            (false, true) => credit.height,
            (false, false) => credit.height + pad.y,
        };
        let safe_top = (height as f32 * safe.top).round() as i32;
        let safe_bottom = (height as f32 * (1.0 - safe.bottom)).round() as i32;
        let top = safe_top + ((safe_bottom - safe_top - panel_height - credit_height) / 2).max(0);

        if !parts.is_empty() {
            self.panels.push(Rect {
                min: IVec2::new(margin, top),
                max: IVec2::new(margin + outer, top + panel_height),
            });
        }
        let mut y = top + pad.y;
        for part in parts {
            let part_height = part.height;
            if part.lines.is_empty() {
                self.strip = Some(Rect {
                    min: IVec2::new(margin + pad.x, y),
                    max: IVec2::new(margin + pad.x + inner, y + part_height),
                });
            } else {
                self.put(part, IVec2::new(centre, y));
            }
            y += part_height + metrics.gap();
        }
        let credit_top = top + panel_height + credit_height - credit.height;
        if !credit.lines.is_empty() {
            self.put(credit, IVec2::new(centre, credit_top));
        }
        if panel_height + credit_height > 0 {
            self.left_px = (margin + outer) as u32;
        }
    }

    /// Adds a block's lines with its top centre at `at`
    fn put(&mut self, block: Block, at: IVec2) {
        self.lines.extend(block.lines.into_iter().map(|line| Line {
            origin: line.origin + at,
            ..line
        }));
    }
}

/// A panel around content `inner_width` wide, centred on `centre`
fn panel_rect(centre: i32, inner_width: i32, top: i32, bottom: i32, pad: IVec2) -> Rect {
    let half = (inner_width + 1) / 2;
    Rect {
        min: IVec2::new(centre - half - pad.x, top),
        max: IVec2::new(centre + half + pad.x, bottom),
    }
}

/// The title, subtitle and date lines
fn heading(text: &OverlayText, style: &OverlayStyle, metrics: &Metrics, max_width: u32) -> Block {
    let font = PixelFont::monogram();
    let (large, small) = (metrics.large, metrics.small);
    let mut block = Vec::new();
    let title = text.title.trim();
    if !title.is_empty() {
        let title_scale = match style.title_size {
            TitleSize::Large => large,
            TitleSize::Small => small,
        };
        let (head, tail) = match &text.subtitle {
            Subtitle::Auto => split_title(font, title, title_scale, max_width),
            Subtitle::Text(subtitle) => (title, subtitle.trim()),
            Subtitle::None => (title, ""),
        };
        for line in font.wrap(head, title_scale, max_width, TITLE_LINES) {
            block.push((line, title_scale, Tone::Text));
        }
        for line in font.wrap(tail, small, max_width, SUBTITLE_LINES) {
            block.push((line, small, Tone::Subtext));
        }
    }
    if let Some(date) = &text.date {
        for line in font.wrap(date, small, max_width, 1) {
            block.push((line, small, Tone::Subtext));
        }
    }
    stack_lines(&block, small)
}

/// Wrapped but never cut short, as the data licences ask for it
fn credit(text: &OverlayText, metrics: &Metrics, max_width: u32) -> Block {
    let font = PixelFont::monogram();
    let lines: Vec<(String, u32, Tone)> = text
        .attribution
        .iter()
        .flat_map(|credit| font.wrap(credit, metrics.small, max_width, usize::MAX))
        .map(|line| (line, metrics.small, Tone::Credit))
        .collect();
    stack_lines(&lines, metrics.small)
}

/// Centred lines, with a gap where the tone changes
fn stack_lines(block: &[(String, u32, Tone)], small: u32) -> Block {
    let font = PixelFont::monogram();
    let mut lines = Vec::new();
    let (mut y, mut bottom, mut width) = (0, 0, 0);
    let mut previous = None;
    for (text, scale, tone) in block {
        if previous.is_some_and(|previous| previous != *tone) {
            y += small as i32;
        }
        let line_width = font.width(text, *scale) as i32;
        width = width.max(line_width);
        lines.push(Line {
            text: text.clone(),
            origin: IVec2::new(-((line_width + 1) / 2), y - (BOX_TOP * scale) as i32),
            scale: *scale,
            tone: *tone,
        });
        bottom = y + (BOX_ROWS * scale) as i32;
        previous = Some(*tone);
        y += (PixelFont::LINE_HEIGHT * scale) as i32;
    }
    Block {
        lines,
        width,
        height: bottom,
    }
}

/// The stats as values over labels in as few rows as fit `max_width`
fn stats_block(stats: &[Stat], metrics: &Metrics, max_width: i32) -> Block {
    if stats.is_empty() {
        return Block::default();
    }
    let font = PixelFont::monogram();
    let (large, small) = (metrics.large, metrics.small);
    let gap = 8 * large as i32;
    let cell_width = |stat: &Stat| {
        value_runs(&stat.value, large, small)
            .1
            .max(font.width(&stat.label, small) as i32)
    };
    let columns_for = |per_row: usize| -> Vec<i32> {
        (0..per_row)
            .map(|column| {
                stats
                    .iter()
                    .skip(column)
                    .step_by(per_row)
                    .map(cell_width)
                    .max()
                    .unwrap_or(0)
            })
            .collect()
    };
    let total = |columns: &[i32]| columns.iter().sum::<i32>() + gap * (columns.len() as i32 - 1);
    let columns = (1..=stats.len())
        .map(|rows| columns_for(stats.len().div_ceil(rows)))
        .find(|columns| total(columns) <= max_width)
        .unwrap_or_else(|| columns_for(1));
    let rows = stats.len().div_ceil(columns.len()) as i32;
    let label_gap = small as i32;
    let cell_height = ((BOX_ROWS * large) as i32) + label_gap + (BOX_ROWS * small) as i32;
    let row_gap = metrics.gap();

    let width = total(&columns);
    let left = -((width + 1) / 2);
    let mut lines = Vec::new();
    for (i, stat) in stats.iter().enumerate() {
        let (row, column) = (i / columns.len(), i % columns.len());
        let x = left + columns[..column].iter().map(|w| w + gap).sum::<i32>();
        let y = row as i32 * (cell_height + row_gap);
        let centred = |width: i32| x + (columns[column] - width) / 2;
        let (runs, value_width) = value_runs(&stat.value, large, small);
        let value_x = centred(value_width);
        let baseline = y + ((BASELINE - BOX_TOP) * large) as i32;
        for (text, scale, offset) in runs {
            lines.push(Line {
                text,
                origin: IVec2::new(value_x + offset, baseline - (BASELINE * scale) as i32),
                scale,
                tone: Tone::Text,
            });
        }
        let label_y = y + (BOX_ROWS * large) as i32 + label_gap;
        lines.push(Line {
            text: stat.label.clone(),
            origin: IVec2::new(
                centred(font.width(&stat.label, small) as i32),
                label_y - (BOX_TOP * small) as i32,
            ),
            scale: small,
            tone: Tone::Subtext,
        });
    }
    Block {
        lines,
        width,
        height: rows * cell_height + (rows - 1) * row_gap,
    }
}

/// An area chart of the profile filling `rect` from its bottom, the walked
/// columns in the route's colour
fn draw_profile(frame: &mut Frame, rect: Rect, profile: &[f32], progress: f32, snap: &mut Snap) {
    let (low, high) = profile
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), &e| {
            (low.min(e), high.max(e))
        });
    let size = rect.max - rect.min;
    let walked = snap.colour(RouteStyle::default().colour);
    let ahead = snap.colour(PROFILE_AHEAD);
    for x in 0..size.x {
        let t = if size.x > 1 {
            x as f32 / (size.x - 1) as f32
        } else {
            0.0
        };
        let share = if high > low {
            (sample(profile, t) - low) / (high - low)
        } else {
            0.0
        };
        let rows = 1 + (share * (size.y - 1) as f32).round() as i32;
        let [r, g, b] = if progress >= 1.0 || t < progress {
            walked
        } else {
            ahead
        };
        for y in 0..rows {
            frame.set_pixel(rect.min.x + x, rect.max.y - 1 - y, [r, g, b, 255]);
        }
    }
}

/// Linear between evenly spaced samples, `t` running from 0 to 1
fn sample(profile: &[f32], t: f32) -> f32 {
    let at = t.clamp(0.0, 1.0) * (profile.len() - 1) as f32;
    let i = (at.floor() as usize).min(profile.len() - 2);
    profile[i] + (profile[i + 1] - profile[i]) * (at - i as f32)
}

/// A stat's value as runs of large figures and small units, each with its
/// offset from the value's left edge, and the value's width: `12.7 km`
/// becomes `12.7` and `km`, `4h 05m` becomes `4`, `h`, `05` and `m`
fn value_runs(value: &str, large: u32, small: u32) -> (Vec<(String, u32, i32)>, i32) {
    let font = PixelFont::monogram();
    let figure = |c: char| c.is_ascii_digit() || matches!(c, '.' | ',' | ':' | '+' | '-');
    let mut runs: Vec<(String, u32, i32)> = Vec::new();
    let mut x = 0;
    let mut spaced = false;
    for c in value.chars() {
        if c.is_whitespace() {
            spaced = true;
            continue;
        }
        let scale = if figure(c) { large } else { small };
        match runs.last_mut() {
            Some((run, run_scale, _)) if *run_scale == scale && !spaced => run.push(c),
            last => {
                if let Some((run, run_scale, offset)) = last {
                    let gap = if spaced { 3 * large } else { large };
                    x = *offset + (font.width(run, *run_scale) + gap) as i32;
                }
                runs.push((c.to_string(), scale, x));
            }
        }
        spaced = false;
    }
    let width = runs.last().map_or(0, |(run, scale, offset)| {
        offset + font.width(run, *scale) as i32
    });
    (runs, width)
}

/// The title as it is if it fits the large lines, otherwise split at its
/// first separator into a title and a subtitle
fn split_title<'a>(
    font: &PixelFont,
    title: &'a str,
    scale: u32,
    max_width: u32,
) -> (&'a str, &'a str) {
    if font.wrap(title, scale, max_width, usize::MAX).len() <= TITLE_LINES {
        return (title, "");
    }
    SEPARATORS
        .iter()
        .filter_map(|sep| title.find(sep).map(|at| (at, sep.len())))
        .filter(|&(at, _)| at > 0)
        .min()
        .map_or((title, ""), |(at, len)| {
            (title[..at].trim(), title[at + len..].trim())
        })
}

/// Colours moved to the nearest in a palette, remembered as they're found
struct Snap<'a> {
    palette: Option<&'a Palette>,
    seen: HashMap<[u8; 3], [u8; 3]>,
}

impl<'a> Snap<'a> {
    fn new(palette: Option<&'a Palette>) -> Snap<'a> {
        Snap {
            palette,
            seen: HashMap::new(),
        }
    }

    fn colour(&mut self, rgb: [u8; 3]) -> [u8; 3] {
        match self.palette {
            None => rgb,
            Some(palette) => *self.seen.entry(rgb).or_insert_with(|| palette.nearest(rgb)),
        }
    }
}

/// Frosted glass: the frame under the panel blurred, tinted and snapped,
/// with its corner pixels left out and a lit top edge
fn draw_panel(frame: &mut Frame, rect: Rect, snap: &mut Snap) {
    let (width, height) = (frame.width() as i32, frame.height() as i32);
    let min = rect.min.max(IVec2::ZERO);
    let max = rect.max.min(IVec2::new(width, height));
    if min.x >= max.x || min.y >= max.y {
        return;
    }
    let blurred = box_blur(frame, min, max, PANEL_BLUR_PX);
    let size = max - min;
    let tint = PANEL_TINT.map(f32::from);
    for y in 0..size.y {
        for x in 0..size.x {
            let (fx, fy) = (min.x + x, min.y + y);
            let corner = (fx == rect.min.x || fx == rect.max.x - 1)
                && (fy == rect.min.y || fy == rect.max.y - 1);
            if corner {
                continue;
            }
            let under = blurred[(y * size.x + x) as usize];
            let mixed: [u8; 3] = std::array::from_fn(|c| {
                (under[c] + (tint[c] - under[c]) * PANEL_TINT_SHARE).round() as u8
            });
            let [r, g, b] = snap.colour(mixed);
            frame.set_pixel(fx, fy, [r, g, b, 255]);
        }
    }
    let [r, g, b] = snap.colour(PANEL_EDGE);
    for x in rect.min.x + 1..rect.max.x - 1 {
        frame.set_pixel(x, rect.min.y, [r, g, b, 255]);
    }
}

/// The frame's colours over `min..max`, each averaged with its neighbours
/// up to `radius` away, reaching outside the box but not the frame
fn box_blur(frame: &Frame, min: IVec2, max: IVec2, radius: i32) -> Vec<[f32; 3]> {
    let (width, height) = (frame.width() as i32, frame.height() as i32);
    // Transparent pixels blur as the tint, so panels over them stay solid
    let rgb = |x: i32, y: i32| {
        let p = frame.pixel(x.clamp(0, width - 1) as u32, y.clamp(0, height - 1) as u32);
        if p[3] < 128 {
            PANEL_TINT.map(f32::from)
        } else {
            [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])]
        }
    };
    let taps = (2 * radius + 1) as f32;
    // Rows first, over the box grown by the radius vertically
    let grown = IVec2::new(min.x, min.y - radius);
    let rows_size = IVec2::new(max.x - min.x, max.y - min.y + 2 * radius);
    let mut rows = vec![[0.0; 3]; (rows_size.x * rows_size.y) as usize];
    for y in 0..rows_size.y {
        for x in 0..rows_size.x {
            let mut sum = [0.0; 3];
            for dx in -radius..=radius {
                let p = rgb(grown.x + x + dx, grown.y + y);
                (0..3).for_each(|c| sum[c] += p[c]);
            }
            rows[(y * rows_size.x + x) as usize] = sum.map(|s| s / taps);
        }
    }
    let size = max - min;
    let mut out = vec![[0.0; 3]; (size.x * size.y) as usize];
    for y in 0..size.y {
        for x in 0..size.x {
            let mut sum = [0.0; 3];
            for dy in 0..=2 * radius {
                let p = rows[((y + dy) * rows_size.x + x) as usize];
                (0..3).for_each(|c| sum[c] += p[c]);
            }
            out[(y * size.x + x) as usize] = sum.map(|s| s / taps);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use isotrail_bundle::Bounds;

    use super::*;
    use crate::Aspect;

    fn story() -> Framing {
        Aspect::Story.framing(3).unwrap()
    }

    fn text(title: &str) -> OverlayText {
        OverlayText {
            title: title.into(),
            subtitle: Subtitle::Auto,
            date: Some("Sunday 15 June 2025".into()),
            stats: vec![
                Stat::new("12.4 km", "distance"),
                Stat::new("846 m", "gain"),
                Stat::new("5h 02m", "time"),
                Stat::new("1085 m", "high point"),
            ],
            profile: Vec::new(),
            attribution: None,
        }
    }

    fn hike_stats() -> HikeStats {
        HikeStats {
            points: 900,
            segments: 1,
            distance_m: 12_000.0,
            gain_m: Some(846.4),
            loss_m: Some(791.0),
            min_ele_m: Some(312.0),
            max_ele_m: Some(1085.0),
            bounds: Bounds {
                min_lat: 0.0,
                min_lon: 0.0,
                max_lat: 0.1,
                max_lon: 0.1,
            },
            start: Some(0),
            end: Some(30 * 3600),
            elapsed_s: Some(30 * 3600 + 25 * 60),
            moving_s: Some(4 * 3600),
        }
    }

    fn layout(text: &OverlayText) -> Overlays {
        Overlays::layout(text, &OverlayStyle::default(), &story())
    }

    #[test]
    fn dates_print_in_each_format() {
        let long = |iso| format_date(iso, DateFormat::Long);
        assert_eq!(long("2025-06-14").unwrap(), "Saturday 14 June 2025");
        assert_eq!(long("2000-02-29").unwrap(), "Tuesday 29 February 2000");
        assert_eq!(long("2026-01-01").unwrap(), "Thursday 1 January 2026");
        assert_eq!(long("1999-12-31").unwrap(), "Friday 31 December 1999");
        assert_eq!(
            format_date("2025-06-04", DateFormat::Short).unwrap(),
            "4 Jun 2025"
        );
        assert_eq!(
            format_date("2025-6-4", DateFormat::Iso).unwrap(),
            "2025-06-04"
        );
        for bad in [
            "2025-02-29",
            "2025-13-01",
            "2025-06-31",
            "2025-06",
            "June",
            "",
        ] {
            for format in [DateFormat::Long, DateFormat::Short, DateFormat::Iso] {
                assert_eq!(format_date(bad, format), None, "{bad}");
            }
        }
    }

    #[test]
    fn durations_and_distances_stay_short() {
        assert_eq!(format_duration(29), "0m");
        assert_eq!(format_duration(45 * 60 + 10), "45m");
        assert_eq!(format_duration(4 * 3600 + 34 * 60 + 40), "4h 35m");
        assert_eq!(format_duration(26 * 3600), "26h 00m");
        assert_eq!(format_elapsed(23 * 3600), "23h 00m");
        assert_eq!(format_elapsed(77 * 3600 + 40 * 60), "3d 6h");
        assert_eq!(format_distance(12_740.0, Units::Metric), "12.7 km");
        assert_eq!(format_distance(101_500.0, Units::Metric), "102 km");
        assert_eq!(format_distance(16_093.44, Units::Imperial), "10.0 mi");
    }

    #[test]
    fn every_stat_has_a_value_and_label() {
        let stats = hike_stats();
        let all = [
            StatKind::Distance,
            StatKind::Gain,
            StatKind::Loss,
            StatKind::Time,
            StatKind::Elapsed,
            StatKind::High,
            StatKind::Low,
            StatKind::Speed,
            StatKind::Pace,
        ];
        let shown = |units| -> Vec<(String, String)> {
            all.iter()
                .map(|kind| kind.stat(&stats, units).unwrap())
                .map(|stat| (stat.value, stat.label))
                .collect()
        };
        let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
            list.iter()
                .map(|(v, l)| (v.to_string(), l.to_string()))
                .collect()
        };
        assert_eq!(
            shown(Units::Metric),
            pairs(&[
                ("12.0 km", "distance"),
                ("846 m", "gain"),
                ("791 m", "loss"),
                ("4h 00m", "time"),
                ("1d 6h", "elapsed"),
                ("1085 m", "high point"),
                ("312 m", "low point"),
                ("3.0 km/h", "speed"),
                ("20:00 /km", "pace"),
            ])
        );
        let imperial = shown(Units::Imperial);
        assert_eq!(imperial[0].0, "7.5 mi");
        assert_eq!(imperial[1].0, "2777 ft");
        assert_eq!(imperial[7].0, "1.9 mph");
        assert_eq!(imperial[8].0, "32:11 /mi");
    }

    #[test]
    fn stats_the_track_lacks_are_left_out() {
        let bare = HikeStats {
            gain_m: None,
            loss_m: None,
            min_ele_m: None,
            max_ele_m: None,
            elapsed_s: None,
            moving_s: None,
            ..hike_stats()
        };
        for kind in [
            StatKind::Gain,
            StatKind::Time,
            StatKind::Elapsed,
            StatKind::Low,
            StatKind::Speed,
            StatKind::Pace,
        ] {
            assert_eq!(kind.stat(&bare, Units::Metric), None, "{kind:?}");
        }
        assert!(StatKind::Distance.stat(&bare, Units::Metric).is_some());
        let standing = HikeStats {
            distance_m: 0.0,
            ..hike_stats()
        };
        assert_eq!(StatKind::Pace.stat(&standing, Units::Metric), None);
        // Elapsed time stands in for moving time
        let unmoving = HikeStats {
            moving_s: None,
            ..hike_stats()
        };
        let time = StatKind::Time.stat(&unmoving, Units::Metric).unwrap();
        assert_eq!(time.value, "30h 25m");
    }

    #[test]
    fn values_split_into_large_figures_and_small_units() {
        let (runs, width) = value_runs("12.7 km", 2, 1);
        assert_eq!(runs, [("12.7".into(), 2, 0), ("km".into(), 1, 46 + 6)]);
        assert_eq!(width, 52 + 11);
        let (runs, _) = value_runs("4h 05m", 2, 1);
        let texts: Vec<(&str, u32)> = runs.iter().map(|(t, s, _)| (t.as_str(), *s)).collect();
        assert_eq!(texts, [("4", 2), ("h", 1), ("05", 2), ("m", 1)]);
        assert_eq!(runs[1].2, 10 + 2);
        let (runs, _) = value_runs("20:00 /km", 2, 1);
        let texts: Vec<(&str, u32)> = runs.iter().map(|(t, s, _)| (t.as_str(), *s)).collect();
        assert_eq!(texts, [("20:00", 2), ("/km", 1)]);
        assert_eq!(value_runs("", 2, 1), (Vec::new(), 0));
    }

    #[test]
    fn short_titles_stay_whole_and_long_ones_split_at_a_separator() {
        let font = PixelFont::monogram();
        let (head, tail) = split_title(font, "Snowdon Horseshoe", 2, 312);
        assert_eq!((head, tail), ("Snowdon Horseshoe", ""));
        let long = "Dolina Pięciu Stawów Polskich - Kozi Wierch | Zawrat";
        assert_eq!(
            split_title(font, long, 2, 312),
            ("Dolina Pięciu Stawów Polskich", "Kozi Wierch | Zawrat")
        );
        let unbroken = "Dolina Pięciu Stawów Polskich przez Kozi Wierch i Zawrat";
        assert_eq!(split_title(font, unbroken, 2, 312), (unbroken, ""));
    }

    fn texts_of(overlays: &Overlays, tone: Tone) -> Vec<&str> {
        overlays
            .lines
            .iter()
            .filter(|line| line.tone == tone)
            .map(|line| line.text.as_str())
            .collect()
    }

    #[test]
    fn a_given_subtitle_or_none_keeps_the_title_whole() {
        let long = "Dolina Pięciu Stawów Polskich - Kozi Wierch | Zawrat i Świnica";
        let subtext = |subtitle: Subtitle| {
            let overlays = layout(&OverlayText {
                subtitle,
                ..text(long)
            });
            texts_of(&overlays, Tone::Subtext)
                .into_iter()
                .take_while(|line| !line.starts_with("Sunday"))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert_eq!(subtext(Subtitle::Auto), ["Kozi Wierch | Zawrat i Świnica"]);
        assert_eq!(subtext(Subtitle::Text("Tatry".into())), ["Tatry"]);
        assert!(subtext(Subtitle::None).is_empty());
        let overlays = layout(&OverlayText {
            subtitle: Subtitle::None,
            ..text(long)
        });
        let title: Vec<&str> = overlays
            .lines
            .iter()
            .filter(|line| line.tone == Tone::Text && line.scale == 2)
            .take(2)
            .map(|line| line.text.as_str())
            .collect();
        assert!(title[1].ends_with('…'), "{title:?}");
    }

    #[test]
    fn a_small_title_uses_the_subtitle_size() {
        let style = OverlayStyle {
            title_size: TitleSize::Small,
            ..OverlayStyle::default()
        };
        let overlays = Overlays::layout(&text("Giewont"), &style, &story());
        let title = overlays
            .lines
            .iter()
            .find(|line| line.text == "Giewont")
            .unwrap();
        assert_eq!(title.scale, 1);
        let default = layout(&text("Giewont"));
        let large = default.lines.iter().find(|line| line.text == "Giewont");
        assert_eq!(large.unwrap().scale, 2);
        assert!(overlays.panels[0].max.y < default.panels[0].max.y);
    }

    #[test]
    fn panels_sit_inside_the_safe_area_and_clear_the_diorama() {
        let framing = story();
        let overlays = layout(&text("Przełęcz pod Łańcuchem | Zażółć gęślą jaźń"));
        assert_eq!(overlays.panels.len(), 2);
        let (title, stats) = (overlays.panels[0], overlays.panels[1]);
        assert_eq!(title.min.y, (640.0_f32 * 0.14).round() as i32);
        assert_eq!(stats.max.y, (640.0_f32 * 0.8).round() as i32);
        for panel in [title, stats] {
            assert!(panel.min.x >= 9 && panel.max.x <= 351, "{panel:?}");
            assert_eq!(panel.min.x + panel.max.x, 360, "centred");
        }
        let diorama = overlays.diorama_framing();
        assert!(diorama.safe.top * 640.0 >= title.max.y as f32);
        assert!((1.0 - diorama.safe.bottom) * 640.0 <= stats.min.y as f32);
        assert_eq!(diorama.safe.left, framing.safe.left);
        // Every line lies inside a panel
        let font = PixelFont::monogram();
        for line in &overlays.lines {
            let (first, last) = font.ink_rows(&line.text).unwrap();
            let top = line.origin.y + (first * line.scale) as i32;
            let bottom = line.origin.y + ((last + 1) * line.scale) as i32;
            let right = line.origin.x + font.width(&line.text, line.scale) as i32;
            assert!(
                overlays.panels.iter().any(|p| p.min.x < line.origin.x
                    && right < p.max.x
                    && p.min.y < top
                    && bottom < p.max.y),
                "{line:?}"
            );
        }
    }

    fn stat_rows(overlays: &Overlays, labels: &[&str]) -> Vec<usize> {
        let mut rows: Vec<(i32, usize)> = Vec::new();
        for line in overlays
            .lines
            .iter()
            .filter(|l| labels.contains(&l.text.as_str()))
        {
            match rows.last_mut() {
                Some((y, count)) if *y == line.origin.y => *count += 1,
                _ => rows.push((line.origin.y, 1)),
            }
        }
        rows.into_iter().map(|(_, count)| count).collect()
    }

    #[test]
    fn stats_take_as_few_rows_as_fit() {
        let labels = ["distance", "gain", "time", "high point"];
        let mut long = text("Ridge");
        long.stats = vec![
            Stat::new("1234.5 mi", "distance"),
            Stat::new("123456 ft", "gain"),
            Stat::new("1234h 40m", "time"),
            Stat::new("12345 ft", "high point"),
        ];
        assert_eq!(stat_rows(&layout(&text("Ridge")), &labels), [4]);
        assert_eq!(stat_rows(&layout(&long), &labels), [2, 2]);
        let mut six = text("Ridge");
        six.stats = ["a", "b", "c", "d", "e", "f"]
            .map(|label| Stat::new("1234 m", label))
            .to_vec();
        let six_labels = ["a", "b", "c", "d", "e", "f"];
        assert_eq!(stat_rows(&layout(&six), &six_labels), [3, 3]);
        six.stats.pop();
        assert_eq!(stat_rows(&layout(&six), &six_labels), [3, 2]);
    }

    #[test]
    fn nothing_to_say_leaves_the_frame_alone() {
        let framing = story();
        let empty = OverlayText {
            title: " ".into(),
            subtitle: Subtitle::Text("not shown without a title".into()),
            date: None,
            stats: Vec::new(),
            profile: vec![300.0],
            attribution: None,
        };
        let overlays = layout(&empty);
        assert!(overlays.panels.is_empty() && overlays.lines.is_empty());
        assert_eq!(overlays.diorama_framing(), framing);
    }

    fn palette_frame(palette: &Palette) -> Frame {
        let framing = story();
        Frame::new(
            framing.width,
            framing.height,
            (0..framing.width * framing.height)
                .flat_map(|i| {
                    let [r, g, b] = palette.colours()[i as usize % palette.colours().len()];
                    [r, g, b, 255]
                })
                .collect(),
        )
    }

    #[test]
    fn panels_and_text_use_only_palette_colours() {
        let palette = Palette::builtin("apollo").unwrap();
        let mut frame = palette_frame(&palette);
        layout(&text("Giewont")).draw(&mut frame, Some(&palette), 1.0);
        assert!(
            frame
                .rgba()
                .chunks(4)
                .all(|p| palette.colours().contains(&[p[0], p[1], p[2]]))
        );
    }

    #[test]
    fn without_panels_only_the_text_and_its_outline_change() {
        let palette = Palette::builtin("apollo").unwrap();
        let before = palette_frame(&palette);
        let mut frame = before.clone();
        let style = OverlayStyle {
            panel: PanelStyle::None,
            ..OverlayStyle::default()
        };
        let overlays = Overlays::layout(&text("Giewont"), &style, &story());
        overlays.draw(&mut frame, Some(&palette), 1.0);
        let ink = [TEXT, SUBTEXT, TEXT_SHADOW].map(|c| palette.nearest(c));
        let mut changed = 0;
        for (now, was) in frame.rgba().chunks(4).zip(before.rgba().chunks(4)) {
            if now != was {
                changed += 1;
                assert!(ink.contains(&[now[0], now[1], now[2]]), "{now:?}");
            }
        }
        assert!(changed > 0);
        // The space the panels would take is still kept clear of the diorama
        assert_eq!(
            overlays.diorama_framing(),
            layout(&text("Giewont")).diorama_framing()
        );
    }

    const CREDIT: &str = "© OpenStreetMap contributors · Terrain Tiles (AWS)";

    fn full_text() -> OverlayText {
        OverlayText {
            profile: (0..64)
                .map(|i| 300.0 + (i as f32 / 6.0).sin() * 80.0)
                .collect(),
            attribution: Some(CREDIT.into()),
            ..text("Przełęcz pod Łańcuchem | Zażółć gęślą jaźń")
        }
    }

    fn blank(framing: &Framing, rgba: [u8; 4]) -> Frame {
        Frame::new(
            framing.width,
            framing.height,
            rgba.repeat((framing.width * framing.height) as usize),
        )
    }

    fn line_box(line: &Line) -> Rect {
        let font = PixelFont::monogram();
        let (first, last) = font.ink_rows(&line.text).unwrap();
        Rect {
            min: IVec2::new(line.origin.x, line.origin.y + (first * line.scale) as i32),
            max: IVec2::new(
                line.origin.x + font.width(&line.text, line.scale) as i32,
                line.origin.y + ((last + 1) * line.scale) as i32,
            ),
        }
    }

    fn inside(inner: Rect, outer: Rect) -> bool {
        outer.min.x <= inner.min.x
            && inner.max.x <= outer.max.x
            && outer.min.y <= inner.min.y
            && inner.max.y <= outer.max.y
    }

    #[test]
    fn the_profile_sits_in_the_stats_panel_and_fills_up_to_the_progress() {
        let framing = story();
        let overlays = layout(&full_text());
        let strip = overlays.strip.unwrap();
        let stats = overlays.panels[1];
        assert!(inside(strip, stats), "{strip:?} {stats:?}");
        let labels = overlays.lines.iter().filter(|l| l.text == "distance");
        assert!(labels.map(line_box).all(|label| label.max.y < strip.min.y));
        assert_eq!(strip.max.y - strip.min.y, 24);

        let mut frame = blank(&framing, [0, 0, 0, 255]);
        overlays.draw(&mut frame, None, 0.5);
        let route = RouteStyle::default().colour;
        let bottom = strip.max.y as u32 - 1;
        let colour = |x: i32| {
            let p = frame.pixel(x as u32, bottom);
            [p[0], p[1], p[2]]
        };
        let width = strip.max.x - strip.min.x;
        assert_eq!(colour(strip.min.x), route);
        assert_eq!(colour(strip.min.x + width / 2 - 2), route);
        assert_eq!(colour(strip.min.x + width / 2 + 2), PROFILE_AHEAD);
        assert_eq!(colour(strip.max.x - 1), PROFILE_AHEAD);
        // The highest sample reaches the top of the strip, the lowest only its bottom row
        let column = |x: i32| {
            (strip.min.y..strip.max.y)
                .filter(|&y| {
                    let p = frame.pixel(x as u32, y as u32);
                    [route, PROFILE_AHEAD].contains(&[p[0], p[1], p[2]])
                })
                .count()
        };
        let heights: Vec<usize> = (strip.min.x..strip.max.x).map(column).collect();
        assert_eq!(heights.iter().max(), Some(&24));
        assert_eq!(heights.iter().min(), Some(&1));
    }

    #[test]
    fn a_flat_profile_is_a_single_row() {
        let framing = story();
        let flat = OverlayText {
            profile: vec![500.0; 10],
            ..text("Flat")
        };
        let overlays = layout(&flat);
        let strip = overlays.strip.unwrap();
        let mut frame = blank(&framing, [0, 0, 0, 255]);
        overlays.draw(&mut frame, None, 1.0);
        let route = RouteStyle::default().colour;
        let row = |y: i32| {
            (strip.min.x..strip.max.x)
                .filter(|&x| frame.pixel(x as u32, y as u32)[..3] == route)
                .count() as i32
        };
        assert_eq!(row(strip.max.y - 1), strip.max.x - strip.min.x);
        assert_eq!(row(strip.max.y - 2), 0);
    }

    #[test]
    fn the_credit_stands_on_the_safe_area_under_the_stats() {
        let framing = story();
        let overlays = layout(&full_text());
        let credit: Vec<&Line> = overlays
            .lines
            .iter()
            .filter(|line| line.tone == Tone::Credit)
            .collect();
        assert_eq!(credit.len(), 1);
        let credit = line_box(credit[0]);
        let safe_bottom = (640.0_f32 * (1.0 - framing.safe.bottom)).round() as i32;
        assert!(credit.max.y <= safe_bottom && credit.max.y >= safe_bottom - 4);
        assert!(overlays.panels[1].max.y < credit.min.y);
        assert!(credit.min.x > 0 && credit.max.x < 360);
        let diorama = overlays.diorama_framing();
        assert!((1.0 - diorama.safe.bottom) * 640.0 <= overlays.panels[1].min.y as f32);

        // A narrower frame wraps it at a space
        let narrow = Overlays::layout(
            &full_text(),
            &OverlayStyle::default(),
            &Aspect::Story.framing(5).unwrap(),
        );
        let credit: Vec<&str> = narrow
            .lines
            .iter()
            .filter(|line| line.tone == Tone::Credit)
            .map(|line| line.text.as_str())
            .collect();
        assert_eq!(credit.len(), 2);
        assert_eq!(credit.join(" "), CREDIT);
    }

    #[test]
    fn landscape_frames_put_everything_in_a_left_column() {
        let framing = Aspect::Wide.framing(3).unwrap();
        let overlays = Overlays::layout(&full_text(), &OverlayStyle::default(), &framing);
        assert_eq!(overlays.panels.len(), 1);
        let column = overlays.panels[0];
        assert!(column.min.x >= 2 && column.max.x <= (640.0 * COLUMN_SHARE) as i32 + 16);
        assert!(inside(overlays.strip.unwrap(), column));
        let (credit, inner): (Vec<&Line>, Vec<&Line>) = overlays
            .lines
            .iter()
            .partition(|line| line.tone == Tone::Credit);
        assert!(inner.iter().all(|line| inside(line_box(line), column)));
        assert!(credit.iter().all(|line| {
            let credit = line_box(line);
            credit.min.y > column.max.y && credit.max.y <= 342
        }));
        assert!(!credit.is_empty());
        // Centred in the safe area's height
        let lowest = credit
            .iter()
            .map(|line| line_box(line).max.y)
            .max()
            .unwrap();
        assert!(
            (column.min.y - 18 - (342 - lowest)).abs() <= 6,
            "{column:?} {lowest}"
        );

        let diorama = overlays.diorama_framing();
        assert!(diorama.safe.left * 640.0 >= column.max.x as f32);
        assert_eq!(
            (diorama.safe.top, diorama.safe.bottom, diorama.safe.right),
            (framing.safe.top, framing.safe.bottom, framing.safe.right)
        );
    }

    #[test]
    fn panels_over_a_transparent_background_are_solid() {
        let palette = Palette::builtin("apollo").unwrap();
        let framing = story();
        let mut frame = blank(&framing, [0, 0, 0, 0]);
        let overlays = layout(&full_text());
        overlays.draw(&mut frame, Some(&palette), 1.0);
        for panel in &overlays.panels {
            let (x, y) = (panel.min.x + 1, panel.max.y - 2);
            let p = frame.pixel(x as u32, y as u32);
            assert_eq!(p[3], 255);
            assert!(palette.colours().contains(&[p[0], p[1], p[2]]), "{p:?}");
        }
        assert_eq!(frame.pixel(0, 0), [0, 0, 0, 0]);
    }
}
