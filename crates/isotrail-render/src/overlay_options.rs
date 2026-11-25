use isotrail_bundle::{HikeStats, SceneBundle, Units};

use crate::{
    DEFAULT_STATS, DateFormat, Fitted, Framing, Labels, Layout, OverlayStyle, OverlayText,
    OverlayTheme, Overlays, PanelStyle, PixelFont, Stat, StatKind, Subtitle, TitleSize,
    data_credit,
};

/// A part of the text printed over the diorama
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayPart {
    Title,
    Date,
    Stats,
    Profile,
    /// No text at all, the data credit and the extra line included
    None,
}

/// What the text over the diorama says and how it looks
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayOptions {
    pub parts: Vec<OverlayPart>,
    /// False leaves the data credit off the image, for the caption
    pub attribution: bool,
    pub panel: PanelStyle,
    /// In place of the hike's name
    pub title: Option<String>,
    pub title_size: TitleSize,
    pub subtitle: Subtitle,
    /// In place of the hike's date
    pub date: Option<String>,
    pub date_format: DateFormat,
    pub stats: Vec<StatKind>,
    pub units: Units,
    pub layout: Layout,
    /// A line of one's own under the data credit
    pub credit_extra: Option<String>,
}

impl Default for OverlayOptions {
    fn default() -> Self {
        OverlayOptions {
            parts: vec![
                OverlayPart::Title,
                OverlayPart::Date,
                OverlayPart::Stats,
                OverlayPart::Profile,
            ],
            attribution: true,
            panel: PanelStyle::default(),
            title: None,
            title_size: TitleSize::default(),
            subtitle: Subtitle::Auto,
            date: None,
            date_format: DateFormat::default(),
            stats: DEFAULT_STATS.to_vec(),
            units: Units::default(),
            layout: Layout::default(),
            credit_extra: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
    #[error("none can't be combined with other parts")]
    NoneWithOthers,

    #[error("{} is listed twice", .0.name())]
    StatTwice(StatKind),
}

/// Text that is drawn, but not all of it
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextWarning {
    /// The track didn't record what the stat needs, so it is left out
    NoStat(StatKind),
    /// Characters the font lacks, left out
    Undrawable { part: TextPart, missing: Vec<char> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextPart {
    Title,
    Subtitle,
    Date,
    Stats,
    CreditLine,
}

/// The text to draw and what it had to leave out
#[derive(Debug, Clone, PartialEq)]
pub struct Composed {
    pub text: OverlayText,
    pub warnings: Vec<TextWarning>,
}

/// Why the text doesn't fit a frame
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DontFit {
    /// isotrail's own words would fit, a theme's don't
    ThemeWords,
    Text,
}

impl OverlayOptions {
    fn shows(&self, part: OverlayPart) -> bool {
        self.parts.contains(&part)
    }

    pub fn check(&self) -> Result<(), TextError> {
        if self.shows(OverlayPart::None) && self.parts.len() > 1 {
            return Err(TextError::NoneWithOthers);
        }
        for (i, stat) in self.stats.iter().enumerate() {
            if self.stats[..i].contains(stat) {
                return Err(TextError::StatTwice(*stat));
            }
        }
        Ok(())
    }

    /// The bundle's text in the words of `labels` with the options applied,
    /// leaving out the parts that weren't asked for
    pub fn text(&self, bundle: &SceneBundle, labels: &Labels) -> Result<Composed, TextError> {
        self.check()?;
        let text = self.compose(bundle, labels);
        let mut warnings = Vec::new();
        if self.shows(OverlayPart::Stats) {
            for &stat in &self.stats {
                if stat.stat(&bundle.meta.stats, self.units).is_none() {
                    warnings.push(TextWarning::NoStat(stat));
                }
            }
        }
        let subtitle = match &text.subtitle {
            Subtitle::Text(subtitle) => subtitle.as_str(),
            _ => "",
        };
        let stats: String = text
            .stats
            .iter()
            .map(|stat| format!("{} {} ", stat.value, stat.label))
            .collect();
        let font = PixelFont::monogram();
        for (part, words) in [
            (TextPart::Title, text.title.as_str()),
            (TextPart::Subtitle, subtitle),
            (TextPart::Date, text.date.as_deref().unwrap_or_default()),
            (TextPart::Stats, stats.as_str()),
            (
                TextPart::CreditLine,
                text.credit_extra.as_deref().unwrap_or_default(),
            ),
        ] {
            let missing = font.missing(words);
            if !missing.is_empty() {
                warnings.push(TextWarning::Undrawable { part, missing });
            }
        }
        Ok(Composed { text, warnings })
    }

    /// The text in isotrail's own words when `labels` changes them, so text
    /// that doesn't fit can be blamed on the changed words
    pub fn built_in_text(&self, bundle: &SceneBundle, labels: &Labels) -> Option<OverlayText> {
        let built_in = Labels::built_in();
        (labels != built_in).then(|| self.compose(bundle, built_in))
    }

    fn compose(&self, bundle: &SceneBundle, labels: &Labels) -> OverlayText {
        let mut text = OverlayText::from_bundle_with(
            bundle,
            self.units,
            &self.stats,
            self.date_format,
            labels,
        );
        if let Some(title) = &self.title {
            text.title.clone_from(title);
        }
        text.subtitle = self.subtitle.clone();
        if let Some(date) = &self.date {
            text.date = Some(date.clone());
            text.short_date = None;
        }

        if !self.shows(OverlayPart::Title) {
            text.title.clear();
        }
        if !self.shows(OverlayPart::Date) {
            text.date = None;
        }
        if !self.shows(OverlayPart::Stats) {
            text.stats.clear();
        }
        if !self.shows(OverlayPart::Profile) {
            text.profile.clear();
        }
        if !self.shows_attribution() {
            text.attribution = None;
        }
        if !self.shows(OverlayPart::None) {
            text.credit_extra = self.credit_extra().map(str::to_owned);
        }
        text
    }

    /// The overlays' look in `theme`'s colours, with the profile walked in
    /// `route_colour`
    pub fn style(&self, theme: &OverlayTheme, route_colour: [u8; 3]) -> OverlayStyle {
        OverlayStyle {
            panel: self.panel,
            title_size: self.title_size,
            layout: self.layout,
            route_colour,
            theme: theme.clone(),
        }
    }

    /// The overlays laid out for a frame, giving parts up to fit it as
    /// [`Overlays::fit`] does. `built_in` is the text in isotrail's own
    /// words, when a theme changed them
    pub fn fit(
        text: &OverlayText,
        built_in: Option<&OverlayText>,
        style: &OverlayStyle,
        framing: &Framing,
    ) -> Result<Fitted, DontFit> {
        Overlays::fit(text, style, framing).map_err(|_| {
            match built_in.filter(|built_in| Overlays::fit(built_in, style, framing).is_ok()) {
                Some(_) => DontFit::ThemeWords,
                None => DontFit::Text,
            }
        })
    }

    /// The stats of [`OverlayOptions::text`] part-way along the hike, with
    /// `walked` holding the figures so far
    pub fn walked_stats(
        &self,
        bundle: &SceneBundle,
        walked: &HikeStats,
        labels: &Labels,
    ) -> Vec<Stat> {
        if !self.shows(OverlayPart::Stats) {
            return Vec::new();
        }
        self.stats
            .iter()
            .filter_map(|stat| {
                stat.stat_walked_with(&bundle.meta.stats, walked, self.units, labels)
            })
            .collect()
    }

    /// The hike's name, as given or as the bundle has it
    pub fn title<'a>(&'a self, bundle: &'a SceneBundle) -> &'a str {
        self.title.as_deref().unwrap_or(&bundle.meta.title)
    }

    /// The extra credit line, unless it is blank
    pub fn credit_extra(&self) -> Option<&str> {
        self.credit_extra
            .as_deref()
            .map(str::trim)
            .filter(|extra| !extra.is_empty())
    }

    /// The data credit to put in the caption, when the image leaves it out,
    /// as asked or because it didn't fit
    pub fn caption_credit(&self, bundle: &SceneBundle, dropped: bool) -> Option<String> {
        let left_off = dropped || !self.shows_attribution();
        data_credit(bundle).filter(|_| left_off)
    }

    fn shows_attribution(&self) -> bool {
        self.attribution && !self.shows(OverlayPart::None)
    }
}

#[cfg(test)]
mod tests {
    use isotrail_bundle::ProfileSample;
    use isotrail_testkit::{diorama_bundle, synthetic_weather};

    use crate::{Aspect, FitStep, ROUTE_COLOUR, Theme};

    use super::*;

    fn bundle() -> SceneBundle {
        let mut bundle = diorama_bundle();
        bundle.meta.title = "Ridge | Valley".into();
        bundle.meta.local_date = Some("2025-06-14".into());
        bundle.meta.attribution = vec!["© Mappers".into(), "Tiles".into()];
        bundle.profile = (0..4)
            .map(|i| ProfileSample {
                distance_m: i as f32 * 100.0,
                ele_m: 400.0 + i as f32,
            })
            .collect();
        let stats = &mut bundle.meta.stats;
        (stats.distance_m, stats.gain_m) = (7_420.0, Some(612.0));
        (stats.moving_s, stats.max_ele_m) = (Some(9_000), Some(842.0));
        bundle
    }

    fn text(options: &OverlayOptions) -> OverlayText {
        options.text(&bundle(), Labels::built_in()).unwrap().text
    }

    #[test]
    fn defaults_print_everything() {
        let options = OverlayOptions::default();
        let composed = options.text(&bundle(), Labels::built_in()).unwrap();
        assert_eq!(composed.warnings, []);
        let text = composed.text;
        assert_eq!(text.title, "Ridge | Valley");
        assert_eq!(text.subtitle, Subtitle::Auto);
        assert_eq!(text.date.as_deref(), Some("Saturday 14 June 2025"));
        let labels: Vec<&str> = text.stats.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["distance", "gain", "time", "high point"]);
        assert_eq!(text.profile, [400.0, 401.0, 402.0, 403.0]);
        assert_eq!(text.attribution.as_deref(), Some("© Mappers · Tiles"));
        assert_eq!(options.caption_credit(&bundle(), false), None);
        assert_eq!(
            options.style(&OverlayTheme::default(), ROUTE_COLOUR),
            OverlayStyle::default()
        );
        assert_eq!(options.title(&bundle()), "Ridge | Valley");
    }

    #[test]
    fn parts_and_overrides_shape_the_text() {
        let only = |parts: &[OverlayPart]| OverlayOptions {
            parts: parts.to_vec(),
            ..OverlayOptions::default()
        };
        let stats = text(&only(&[OverlayPart::Stats]));
        assert!(stats.title.is_empty() && stats.date.is_none() && stats.profile.is_empty());
        assert_eq!(stats.stats.len(), 4);
        assert!(stats.attribution.is_some());
        let none = OverlayOptions {
            credit_extra: Some("me".into()),
            ..only(&[OverlayPart::None])
        };
        let nothing = text(&none);
        assert!(nothing.title.is_empty() && nothing.stats.is_empty());
        assert_eq!((nothing.attribution, nothing.credit_extra), (None, None));
        assert_eq!(
            none.caption_credit(&bundle(), false).as_deref(),
            Some("© Mappers · Tiles")
        );

        let given = OverlayOptions {
            title: Some("Łańcuch".into()),
            subtitle: Subtitle::None,
            date: Some("Summer 2025".into()),
            credit_extra: Some("  ".into()),
            ..OverlayOptions::default()
        };
        let text = text(&given);
        assert_eq!(text.title, "Łańcuch");
        assert_eq!(text.subtitle, Subtitle::None);
        assert_eq!(text.date.as_deref(), Some("Summer 2025"));
        assert_eq!(text.short_date, None);
        assert_eq!(text.credit_extra, None);
        assert_eq!(given.title(&bundle()), "Łańcuch");
    }

    #[test]
    fn mistakes_and_gaps_come_back_as_values() {
        let options = OverlayOptions {
            parts: vec![OverlayPart::None, OverlayPart::Title],
            ..OverlayOptions::default()
        };
        assert_eq!(options.check(), Err(TextError::NoneWithOthers));
        let options = OverlayOptions {
            stats: vec![StatKind::Gain, StatKind::Time, StatKind::Gain],
            ..OverlayOptions::default()
        };
        let error = options.text(&bundle(), Labels::built_in()).unwrap_err();
        assert_eq!(error, TextError::StatTwice(StatKind::Gain));
        assert_eq!(error.to_string(), "gain is listed twice");

        let options = OverlayOptions {
            stats: vec![StatKind::Pace, StatKind::Low, StatKind::Distance],
            title: Some("Ridge ✓".into()),
            ..OverlayOptions::default()
        };
        let composed = options.text(&bundle(), Labels::built_in()).unwrap();
        let labels: Vec<&str> = composed
            .text
            .stats
            .iter()
            .map(|s| s.label.as_str())
            .collect();
        assert_eq!(labels, ["pace", "distance"]);
        assert_eq!(
            composed.warnings,
            [
                TextWarning::NoStat(StatKind::Low),
                TextWarning::Undrawable {
                    part: TextPart::Title,
                    missing: vec!['✓'],
                },
            ]
        );
    }

    #[test]
    fn the_caption_takes_the_credit_with_the_weather() {
        let mut with_weather = bundle();
        with_weather.weather = Some(synthetic_weather(1_750_064_400, 3));
        let off = OverlayOptions {
            attribution: false,
            ..OverlayOptions::default()
        };
        assert_eq!(
            off.caption_credit(&with_weather, false).as_deref(),
            Some("© Mappers · Tiles · Weather data by Open-Meteo.com")
        );
        assert_eq!(
            OverlayOptions::default()
                .caption_credit(&bundle(), true)
                .as_deref(),
            Some("© Mappers · Tiles")
        );
    }

    #[test]
    fn a_coarse_frame_gives_parts_up_or_blames_the_theme() {
        let options = OverlayOptions {
            stats: vec![
                StatKind::Distance,
                StatKind::Gain,
                StatKind::Time,
                StatKind::High,
                StatKind::Low,
                StatKind::Speed,
            ],
            ..OverlayOptions::default()
        };
        let mut bundle = bundle();
        bundle.meta.stats.min_ele_m = Some(300.0);
        let text = options.text(&bundle, Labels::built_in()).unwrap().text;
        let style = OverlayStyle::default();
        let fit = |text: &OverlayText, built_in: Option<&OverlayText>, aspect: Aspect, scale| {
            OverlayOptions::fit(text, built_in, &style, &aspect.framing(scale).unwrap())
        };
        assert_eq!(fit(&text, None, Aspect::Story, 3).unwrap().steps, []);
        let coarse = fit(&text, None, Aspect::Story, 10).unwrap();
        assert!(coarse.credit_dropped());
        assert!(matches!(
            coarse.steps.last(),
            Some(FitStep::LeftOutStats(_))
        ));

        let mut long = text.clone();
        long.stats[0] = Stat::new("123456789012345 km", "distance");
        assert_eq!(
            fit(&long, None, Aspect::Square, 10).unwrap_err(),
            DontFit::Text
        );
        assert_eq!(
            fit(&long, Some(&text), Aspect::Square, 10).unwrap_err(),
            DontFit::ThemeWords
        );
        assert!(
            options
                .built_in_text(&bundle, &Theme::default().labels)
                .is_none()
        );
    }
}
