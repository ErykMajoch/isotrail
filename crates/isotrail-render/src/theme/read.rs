use std::ops::RangeInclusive;

use glam::Vec3;
use toml::{Table, Value};

use super::{Field, Theme, ThemeError, Visitor, tables};
use crate::labels::placeholders;
use crate::{SUN_AZIMUTH_RANGE, SUN_ELEVATION_RANGE, parse_colour};

pub(super) fn read(text: &str) -> Result<Theme, ThemeError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let table: Table = text
        .parse()
        .map_err(|error: toml::de::Error| ThemeError::Toml(error.to_string().trim().to_owned()))?;
    let mut theme = Theme::default();
    let mut reader = Reader {
        tables: vec![(String::new(), table)],
        error: None,
    };
    tables::visit(&mut theme, &mut reader);
    if let Some(error) = reader.error {
        return Err(error);
    }
    if let Some((_, rest)) = reader.tables.pop()
        && let Some((key, value)) = rest.into_iter().next()
    {
        return Err(match value {
            Value::Table(_) => ThemeError::UnknownTable(key),
            _ => ThemeError::OutsideTable { key },
        });
    }
    Ok(theme)
}

/// Takes each key from the file as the tables visit it, so whatever is left
/// over is unknown
struct Reader {
    /// The tables being read, outermost first, each with its dotted name
    tables: Vec<(String, Table)>,
    error: Option<ThemeError>,
}

impl Reader {
    fn take(&mut self, key: &str) -> Option<(String, Value)> {
        if self.error.is_some() {
            return None;
        }
        let (path, table) = self.tables.last_mut()?;
        let value = table.remove(key)?;
        let name = if path.is_empty() {
            key.to_owned()
        } else {
            format!("{path}.{key}")
        };
        Some((name, value))
    }
}

impl Visitor for Reader {
    fn field(&mut self, key: &'static str, field: Field) {
        let Some((name, value)) = self.take(key) else {
            return;
        };
        if let Err(message) = set(field, &value) {
            self.error = Some(ThemeError::Value { key: name, message });
        }
    }

    fn table(
        &mut self,
        name: &'static str,
        _comment: &'static str,
        body: &mut dyn FnMut(&mut dyn Visitor),
    ) {
        let Some((path, value)) = self.take(name) else {
            return;
        };
        let Value::Table(table) = value else {
            self.error = Some(ThemeError::Value {
                key: path,
                message: "expected a table".into(),
            });
            return;
        };
        self.tables.push((path, table));
        body(self);
        let Some((path, rest)) = self.tables.pop() else {
            return;
        };
        if self.error.is_none()
            && let Some((key, value)) = rest.into_iter().next()
        {
            self.error = Some(match value {
                Value::Table(_) => ThemeError::UnknownTable(format!("{path}.{key}")),
                _ => ThemeError::UnknownKey { table: path, key },
            });
        }
    }
}

fn set(field: Field, value: &Value) -> Result<(), String> {
    match field {
        Field::Colour(rgb) => *rgb = colour(value)?,
        Field::Rgba(rgba) => {
            let [r, g, b] = colour(value)?;
            *rgba = [r, g, b, rgba[3]];
        }
        Field::Rgbas(list) => {
            let values = array(value, Some(list.len()))?;
            for (rgba, value) in list.iter_mut().zip(values) {
                let [r, g, b] = colour(value)?;
                *rgba = [r, g, b, rgba[3]];
            }
        }
        Field::Light(rgb) => *rgb = light(value)?,
        Field::Number(number, range) => *number = within(single(value)?, &range)?,
        Field::Seconds(number, range) => {
            let x = float(value)?;
            if !range.contains(&x) {
                return Err(between(&range));
            }
            *number = x;
        }
        Field::Whole(number, range) => *number = whole(value, &range)?,
        Field::Wholes(list, range) => {
            let values = array(value, Some(list.len()))?;
            for (number, value) in list.iter_mut().zip(values) {
                *number = whole(value, &range)?;
            }
        }
        Field::Sun(sun) => {
            let values = array(value, Some(2))?;
            let azimuth = within(single(&values[0])?, &SUN_AZIMUTH_RANGE)
                .map_err(|e| format!("bearing {e}"))?;
            let elevation = within(single(&values[1])?, &SUN_ELEVATION_RANGE)
                .map_err(|e| format!("elevation {e}"))?;
            *sun = [azimuth.into(), elevation.into()];
        }
        Field::Text(text, allowed) => {
            let new = string(value)?;
            if !allowed.is_empty()
                && let Some(unknown) = placeholders(new).find(|name| !allowed.contains(name))
            {
                let known: Vec<String> = allowed.iter().map(|name| format!("{{{name}}}")).collect();
                return Err(format!(
                    "unknown placeholder {{{unknown}}}; this pattern takes {}",
                    known.join(" ")
                ));
            }
            new.clone_into(text);
        }
        Field::Texts(list) => {
            let values = array(value, Some(list.len()))?;
            for (text, value) in list.iter_mut().zip(values) {
                string(value)?.clone_into(text);
            }
        }
        Field::TextList(list) => {
            *list = array(value, None)?
                .iter()
                .map(|value| string(value).map(str::to_owned))
                .collect::<Result<_, _>>()?;
        }
    }
    Ok(())
}

fn colour(value: &Value) -> Result<[u8; 3], String> {
    value
        .as_str()
        .ok_or_else(|| {
            format!(
                "expected a colour such as \"#d64834\", found {}",
                kind(value)
            )
        })
        .and_then(parse_colour)
}

fn light(value: &Value) -> Result<Vec3, String> {
    if let Some(text) = value.as_str() {
        return parse_colour(text).map(|rgb| Vec3::from_array(rgb.map(|c| f32::from(c) / 255.0)));
    }
    let values = array(value, Some(3)).map_err(|_| {
        format!(
            "expected [red, green, blue] strengths such as [1.0, 0.96, 0.88], or \"#rrggbb\", found {}",
            kind(value)
        )
    })?;
    let mut rgb = [0.0; 3];
    for (channel, value) in rgb.iter_mut().zip(values) {
        *channel = within(single(value)?, &(0.0..=4.0))?;
    }
    Ok(Vec3::from_array(rgb))
}

fn array(value: &Value, length: Option<usize>) -> Result<&[Value], String> {
    let Value::Array(values) = value else {
        return Err(match length {
            Some(n) => format!("expected a list of {n}, found {}", kind(value)),
            None => format!("expected a list, found {}", kind(value)),
        });
    };
    match length {
        Some(n) if values.len() != n => Err(format!(
            "expected a list of {n}, found {} item{}",
            values.len(),
            if values.len() == 1 { "" } else { "s" }
        )),
        _ => Ok(values),
    }
}

fn string(value: &Value) -> Result<&str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("expected text in quotes, found {}", kind(value)))
}

fn float(value: &Value) -> Result<f64, String> {
    match value {
        Value::Float(x) if x.is_finite() => Ok(*x),
        Value::Float(_) => Err("expected a finite number".into()),
        Value::Integer(n) => Ok(*n as f64),
        _ => Err(format!("expected a number, found {}", kind(value))),
    }
}

/// The number written, as near as single precision gets to it
fn single(value: &Value) -> Result<f32, String> {
    let x = float(value)?;
    x.to_string()
        .parse()
        .map_err(|_| format!("expected a number, found {x}"))
}

fn within(x: f32, range: &RangeInclusive<f32>) -> Result<f32, String> {
    if range.contains(&x) {
        Ok(x)
    } else {
        Err(between(range))
    }
}

fn whole(value: &Value, range: &RangeInclusive<u32>) -> Result<u32, String> {
    let Value::Integer(n) = value else {
        return Err(format!("expected a whole number, found {}", kind(value)));
    };
    u32::try_from(*n)
        .ok()
        .filter(|n| range.contains(n))
        .ok_or_else(|| between(range))
}

fn between<T: std::fmt::Display>(range: &RangeInclusive<T>) -> String {
    format!("must be between {} and {}", range.start(), range.end())
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "text",
        Value::Integer(_) | Value::Float(_) => "a number",
        Value::Boolean(_) => "true or false",
        Value::Datetime(_) => "a date",
        Value::Array(_) => "a list",
        Value::Table(_) => "a table",
    }
}
