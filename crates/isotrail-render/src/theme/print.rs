use std::fmt::Write;

use glam::Vec3;

use super::{Field, Theme, Visitor, tables};
use crate::colour_name;

const HEADER: &str = "\
# isotrail theme: colours, words and fine-tuning for render, embed and style-lab
# Every value here is the built-in one. Keep the keys you change and delete
# the rest, because a missing key keeps its built-in value. Colours such as
# \"#d64834\" move to the nearest colour of the palette, like everything drawn
";

pub(super) fn print(theme: &Theme) -> String {
    let mut printer = Printer {
        out: HEADER.to_owned(),
        path: Vec::new(),
    };
    tables::visit(&mut theme.clone(), &mut printer);
    printer.out
}

struct Printer {
    out: String,
    path: Vec<&'static str>,
}

impl Visitor for Printer {
    fn field(&mut self, key: &'static str, field: Field) {
        let value = match field {
            Field::Colour(rgb) => quoted(&colour_name(*rgb)),
            Field::Rgba(rgba) => quoted(&rgb_name(rgba)),
            Field::Rgbas(list) => list_of(list.iter().map(|rgba| quoted(&rgb_name(rgba)))),
            Field::Light(rgb) => light(*rgb),
            Field::Number(x, _) => format!("{x:?}"),
            Field::Seconds(x, _) => format!("{x:?}"),
            Field::Whole(n, _) => n.to_string(),
            Field::Wholes(list, _) => list_of(list.iter().map(u32::to_string)),
            Field::Sun(sun) => list_of(sun.iter().map(|x| format!("{x:?}"))),
            Field::Text(text, _) => quoted(text),
            Field::Texts(list) => list_of(list.iter().map(|text| quoted(text))),
            Field::TextList(list) => list_of(list.iter().map(|text| quoted(text))),
        };
        let _ = writeln!(self.out, "{key} = {value}");
    }

    fn note(&mut self, line: &'static str) {
        let _ = writeln!(self.out, "# {line}");
    }

    fn table(
        &mut self,
        name: &'static str,
        comment: &'static str,
        body: &mut dyn FnMut(&mut dyn Visitor),
    ) {
        self.path.push(name);
        self.out.push('\n');
        for line in comment.lines() {
            let _ = writeln!(self.out, "# {line}");
        }
        let _ = writeln!(self.out, "[{}]", self.path.join("."));
        body(self);
        self.path.pop();
    }
}

fn rgb_name(&[r, g, b, _]: &[u8; 4]) -> String {
    colour_name([r, g, b])
}

fn light(rgb: Vec3) -> String {
    list_of(rgb.to_array().iter().map(|x| format!("{x:?}")))
}

fn list_of(items: impl Iterator<Item = String>) -> String {
    format!("[{}]", items.collect::<Vec<_>>().join(", "))
}

fn quoted(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
}
