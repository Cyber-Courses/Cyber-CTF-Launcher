//! The webview's English catalogue (`src/messages/en`), which the core says its coded messages
//! from too: the log, an operation's ✗ line and the core's own messages say a code's sentence as the
//! English UI shows it, from the one place it is written.

use std::sync::LazyLock;

use serde::Serialize;
use serde_json::Value;

/// `errors.json`: the sentences of the error codes (`errors.codes.*`).
pub(crate) static ERRORS: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../src/messages/en/errors.json")));

/// `labs.json`: the lab page's sentences (`labs.failure.portHeldBy`, said after a port clash).
pub(crate) static LABS: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../src/messages/en/labs.json")));

/// `machine.json`: the Machine page's sentences (`machine.unavailable.*`, why a provider can't run).
pub(crate) static MACHINE: LazyLock<Value> = LazyLock::new(|| parse(include_str!("../../src/messages/en/machine.json")));

/// A catalogue file's JSON; Null for one that isn't JSON (a test reads every one), whose
/// sentences are then said by their key.
fn parse(source: &str) -> Value {
    serde_json::from_str(source).unwrap_or(Value::Null)
}

/// The sentence of `coded`, a value that goes to the webview as `{ "code": …, "args": {…} }`:
/// the entry of `section` named by its code, filled in with its args, as the English UI says it.
pub(crate) fn coded(section: &Value, coded: &impl Serialize) -> String {
    let wire = serde_json::to_value(coded).unwrap_or_default();
    let key = wire["code"].as_str().unwrap_or_default();
    sentence(section, key, &wire["args"])
}

/// The entry `key` of `section`, filled in with `args` (an object, or Null for none): its
/// `{name}` placeholders replaced by the values, a plural (`{ "one", "other" }`) said in the form
/// its `count` takes in English. An entry that isn't there is said by its key.
pub(crate) fn sentence(section: &Value, key: &str, args: &Value) -> String {
    let entry = &section[key];
    let form = if args["count"] == 1 { "one" } else { "other" };
    let Some(text) = entry.as_str().or_else(|| entry[form].as_str()) else {
        return key.to_owned();
    };
    let mut text = text.to_owned();
    for (name, value) in args.as_object().into_iter().flatten() {
        let value = value.as_str().map_or_else(|| value.to_string(), str::to_owned);
        text = text.replace(&format!("{{{name}}}"), &value);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_catalogue_file_reads_as_json() {
        for file in [&*ERRORS, &*LABS, &*MACHINE] {
            assert!(file.is_object(), "an English catalogue file isn't a JSON object");
        }
    }

    #[test]
    fn a_sentence_is_filled_in_and_a_plural_said_in_the_form_of_its_count() {
        let section = json!({ "port": "Port {port} of {host}.", "labs": { "one": "{count} lab.", "other": "{count} labs." } });
        assert_eq!(sentence(&section, "port", &json!({ "port": 8080, "host": "pve" })), "Port 8080 of pve.");
        assert_eq!(sentence(&section, "labs", &json!({ "count": 1 })), "1 lab.");
        assert_eq!(sentence(&section, "labs", &json!({ "count": 3 })), "3 labs.");
        assert_eq!(sentence(&section, "port", &json!({ "port": "22" })), "Port 22 of {host}.");
    }

    #[test]
    fn an_entry_that_isnt_there_is_said_by_its_key() {
        assert_eq!(sentence(&json!({}), "missing", &Value::Null), "missing");
    }
}
