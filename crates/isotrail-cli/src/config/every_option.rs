use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};
use isotrail_options::{Kind, OptionInfo, Settings, describe, reason_alone, render_command};
use serde_json::json;
use toml::Value;

use super::{UserFile, matches, parse_args, with_config};
use crate::{Cli, Command};

const HIKE: &str = "hike.gpx";
const JSON_PLACE: &str = "settings.json";

/// A value each option without fixed words or a range takes, and one it
/// refuses, if any
const SAMPLES: [(&str, &str, Option<&str>); 17] = [
    ("size", "800x600", Some("1x1")),
    ("safe-area", "10", Some("50")),
    ("time", "07:30", Some("25:00")),
    ("theme", "dusk.toml", None),
    ("sun", "200,-5", Some("400,0")),
    ("route-colour", "#ff0000", Some("red")),
    ("title", "Ridge", None),
    ("date", "June", None),
    ("credit-extra", "Jo", None),
    ("ffmpeg", "tools/ffmpeg", None),
    (
        "overpass-url",
        "https://overpass.example/api/interpreter",
        None,
    ),
    (
        "terrain-url",
        "https://tiles.example/{z}/{x}/{y}.png",
        Some("ftp://tiles.example/{z}/{x}/{y}.png"),
    ),
    (
        "weather-url",
        "https://weather.example",
        Some("weather.example"),
    ),
    ("cache-dir", "cache", None),
    ("padding", "10,200", Some("200")),
    ("trim-start", "300m", Some("-5")),
    ("trim-end", "1.5km", Some("1.5mi")),
];

/// One value written the three ways
struct Given {
    flag: String,
    toml: Value,
    json: serde_json::Value,
}

impl Given {
    fn word(name: &str, word: &str, list: bool) -> Given {
        let (toml, json) = if list {
            (
                Value::Array(vec![Value::String(word.to_owned())]),
                json!([word]),
            )
        } else {
            (Value::String(word.to_owned()), json!(word))
        };
        Given {
            flag: format!("--{name}={word}"),
            toml,
            json,
        }
    }

    fn number(name: &str, number: f64) -> Given {
        let (toml, json) = if number.fract() == 0.0 {
            (Value::Integer(number as i64), json!(number as i64))
        } else {
            (Value::Float(number), json!(number))
        };
        Given {
            flag: format!("--{name}={number}"),
            toml,
            json,
        }
    }
}

fn sample(name: &str) -> Option<(&'static str, Option<&'static str>)> {
    SAMPLES
        .iter()
        .find(|(option, ..)| *option == name)
        .map(|&(_, good, bad)| (good, bad))
}

/// A value that differs from the default
fn good(option: &OptionInfo) -> Given {
    let name = &option.name;
    let default = option.default.as_deref();
    match option.kind {
        Kind::Switch => Given {
            flag: format!("--{name}"),
            toml: Value::Boolean(true),
            json: json!(true),
        },
        Kind::Choice | Kind::List if !option.choices.is_empty() => {
            let word = option
                .choices
                .iter()
                .find(|&choice| Some(choice.as_str()) != default)
                .unwrap();
            Given::word(name, word, option.kind == Kind::List)
        }
        Kind::Number => {
            let (min, max) = option.range.unwrap();
            let usual = default == Some(min.to_string().as_str());
            Given::number(name, if usual { max } else { min })
        }
        _ => {
            let (good, _) = sample(name).unwrap_or_else(|| panic!("no sample for --{name}"));
            Given::word(name, good, false)
        }
    }
}

/// A value the option refuses, if there is one it can be given
fn bad(option: &OptionInfo) -> Option<Given> {
    let name = &option.name;
    let given = match option.kind {
        Kind::Switch => return None,
        Kind::Number => {
            let (_, max) = option.range?;
            Given::number(name, max + 1.0)
        }
        Kind::Choice | Kind::List if !option.choices.is_empty() => {
            Given::word(name, "nope", option.kind == Kind::List)
        }
        _ => Given::word(name, sample(name)?.1?, false),
    };
    reason_alone(&render_command(), &given.flag).map(|_| given)
}

fn line() -> Vec<OsString> {
    ["isotrail", "render", HIKE]
        .into_iter()
        .map(OsString::from)
        .collect()
}

fn config_file(dir: &Path, name: &str, value: &Value) -> PathBuf {
    let path = dir.join(format!("{name}.toml"));
    fs::write(&path, format!("[render]\n{name} = {value}\n")).unwrap();
    path
}

fn json_text(name: &str, value: &serde_json::Value) -> String {
    json!({ name: value }).to_string()
}

fn settled(cli: Cli) -> String {
    match cli.command {
        Command::Render(args) => format!("{:?}", args.options),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_flag_a_config_key_and_json_settle_alike() {
    let dir = tempfile::tempdir().unwrap();
    for option in describe() {
        let name = &option.name;
        let given = good(&option);
        let mut typed = line();
        typed.push(OsString::from(&given.flag));
        let by_flag = settled(Cli::try_parse_from(typed).unwrap());
        let user = UserFile {
            path: config_file(dir.path(), name, &given.toml),
            named: false,
        };
        let by_config = settled(parse_args(
            with_config(line(), &matches(&line()), Some(user)).unwrap(),
        ));
        let by_json = Settings::from_json(&json_text(name, &given.json), JSON_PLACE)
            .and_then(|settings| settings.options())
            .map(|options| format!("{options:?}"))
            .unwrap();
        assert_eq!(by_config, by_flag, "--{name} from a config file");
        assert_eq!(by_json, by_flag, "--{name} from JSON");
        let usual = settled(Cli::try_parse_from(line()).unwrap());
        assert_ne!(by_flag, usual, "--{name} {}", given.flag);
    }
}

/// Why the command line refuses a flag, as a config file words it
fn flag_reason(flag: &str) -> String {
    let mut typed = line();
    typed.push(OsString::from(flag));
    let error = Cli::try_parse_from(typed).unwrap_err();
    match std::error::Error::source(&error) {
        Some(reason) if error.kind() == ErrorKind::ValueValidation => reason.to_string(),
        _ => {
            let message = error.to_string();
            let first = message.lines().next().unwrap_or_default();
            first.trim_start_matches("error: ").to_owned()
        }
    }
}

#[test]
fn a_bad_value_gets_the_same_reason_three_ways() {
    let dir = tempfile::tempdir().unwrap();
    let mut checked = 0;
    for option in describe() {
        let name = &option.name;
        let Some(given) = bad(&option) else { continue };
        let by_flag = flag_reason(&given.flag);
        let path = config_file(dir.path(), name, &given.toml);
        let user = UserFile {
            path: path.clone(),
            named: false,
        };
        let error = with_config(line(), &matches(&line()), Some(user)).unwrap_err();
        let message = format!("{error:#}");
        let prefix = format!("{name} in {}: ", path.display());
        let by_config = message.strip_prefix(&prefix).unwrap_or(&message);
        let error = Settings::from_json(&json_text(name, &given.json), JSON_PLACE).unwrap_err();
        let message = format!("{error:#}");
        let prefix = format!("{name} in {JSON_PLACE}: ");
        let by_json = message.strip_prefix(&prefix).unwrap_or(&message);
        assert_eq!(by_config, by_flag, "--{name}");
        assert_eq!(by_json, by_flag, "--{name}");
        checked += 1;
    }
    // Every option with words, a range or a refused sample
    assert!(checked >= 50, "only {checked} options checked");
}

/// The names in a JavaScript list such as `const LOOK = ['a', 'b'];`
fn script_list(script: &str, name: &str) -> Vec<String> {
    let start = script.find(&format!("const {name} = [")).unwrap();
    let end = start + script[start..].find("];").unwrap();
    script[start..end]
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// `blockHeight` as the attribute `block-height`
fn kebab(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn the_embeds_own_names_are_options() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("web/isotrail-embed.js"))
        .unwrap()
        .replace("\r\n", "\n");
    let reader = fs::read_to_string(root.join("crates/isotrail-web/src/options.rs")).unwrap();
    let render = render_command();
    let mut cli = Cli::command();
    cli.build();
    let embed = cli.find_subcommand("embed").unwrap();
    let names: BTreeSet<&str> = render
        .get_arguments()
        .chain(embed.get_arguments())
        .flat_map(|arg| {
            arg.get_long()
                .into_iter()
                .chain(arg.get_all_aliases().unwrap_or_default())
        })
        .collect();
    let look = script_list(&script, "LOOK");
    assert!(look.len() > 20, "{look:?}");
    let render_names: BTreeSet<&str> = render
        .get_arguments()
        .flat_map(|arg| {
            arg.get_long()
                .into_iter()
                .chain(arg.get_all_aliases().unwrap_or_default())
        })
        .collect();
    for name in &look {
        assert!(render_names.contains(name.as_str()), "LOOK names {name}");
    }
    let read: Vec<String> = reader
        .split("(options, \"")
        .skip(1)
        .map(|rest| kebab(rest.split('"').next().unwrap_or_default()))
        .collect();
    assert!(read.len() > 20, "{read:?}");
    for name in &read {
        assert!(names.contains(name.as_str()), "the reader takes {name}");
    }
}
