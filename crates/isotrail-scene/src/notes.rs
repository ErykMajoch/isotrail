use std::fmt;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Level, Metadata, Subscriber};

/// Where the warnings about the track itself come from, which the command
/// line prints as it reads and builds
const FROM: [&str; 2] = ["isotrail_track", "isotrail_geo::scene"];

/// Runs `work` and gives back what it warned about the track, in order and
/// each once
pub(crate) fn heard_while<T>(work: impl FnOnce() -> T) -> (T, Vec<String>) {
    let notes = Notes::default();
    let result = tracing::subscriber::with_default(notes.clone(), work);
    let heard = notes.0.lock().map(|kept| kept.clone()).unwrap_or_default();
    (result, heard)
}

#[derive(Clone, Default)]
struct Notes(Arc<Mutex<Vec<String>>>);

impl Subscriber for Notes {
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.is_event()
            && *metadata.level() <= Level::WARN
            && FROM.iter().any(|from| metadata.target().starts_with(from))
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut message = Message::default();
        event.record(&mut message);
        if let Ok(mut kept) = self.0.lock()
            && !message.0.is_empty()
            && !kept.contains(&message.0)
        {
            kept.push(message.0);
        }
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

#[derive(Default)]
struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}
