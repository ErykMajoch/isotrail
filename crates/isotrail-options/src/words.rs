/// A message without the operating system's error number, its full stop or
/// a capital letter at the start of a sentence. A file name or path at the
/// start, such as `Ridge.gpx`, keeps its capital
pub fn plain(text: &str) -> String {
    let text = match text.rfind(" (os error ") {
        Some(at) if text.ends_with(')') => &text[..at],
        _ => text,
    };
    let text = text.trim().trim_end_matches('.');
    let word = text.split(' ').next().is_some_and(|word| {
        word.chars()
            .all(|c| c.is_alphabetic() || c == '\'' || c == '’')
    });
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if word && first.is_uppercase() && second.is_lowercase() => {
            first
                .to_lowercase()
                .chain(text[first.len_utf8()..].chars())
                .collect()
        }
        _ => text.to_owned(),
    }
}
