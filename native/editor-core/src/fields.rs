//! The typed component forms. This covers the data fields the engine reports for each component
//! (`component.fields`, in `src/Editor.Kernel/ComponentFields.cs`), the text each kind of widget
//! shows, and turning what was typed back into the JSON of the component's data. All pure
//! functions. The session keeps the forms of the selected entity (`session/entity.rs`).

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{json, Value};

/// The kind of widget a data field takes, as the engine names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Bool,
    Int,
    Float,
    String,
    /// One of `DataField::names`.
    Enum,
    /// Any of `DataField::names`: a list of them, or one text of them with commas.
    Flags,
    /// #RRGGBBAA.
    Color,
    /// Two numbers, written "x,y".
    Vector2,
    /// The id of a prototype of `DataField::prototype_kind`.
    Prototype,
    /// Another entity's uid, or "invalid" for none.
    Entity,
    /// Seconds.
    Timespan,
    /// Anything else: lists, maps, nested data. Edited as JSON text.
    #[serde(other)]
    Other,
}

/// One data field of a component, as the engine told it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataField {
    /// Its name in the map file.
    pub tag: String,
    pub kind: FieldKind,
    /// The C# type's name, for the mapper to read.
    #[serde(rename = "type", default)]
    pub type_name: String,
    /// The value now, as the component text has it. Null for none.
    #[serde(default)]
    pub value: Value,
    /// The prototype's value, or a new component's when the prototype lacks the component.
    #[serde(default)]
    pub prototype: Value,
    /// The game reads it from a prototype and never writes it.
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub names: Option<Vec<String>>,
    #[serde(default)]
    pub prototype_kind: Option<String>,
}

impl DataField {
    /// An enum's or a flag set's names. Empty for other kinds.
    pub fn names(&self) -> &[String] {
        self.names.as_deref().unwrap_or_default()
    }
}

/// One component's data fields.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ComponentFields {
    pub name: String,
    pub fields: Vec<DataField>,
}

/// The engine's answer to `component.fields`: the fields of each component, and the ids of
/// each prototype kind the fields name that it had not told before.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct FieldsAnswer {
    pub components: Vec<ComponentFields>,
    #[serde(default)]
    pub ids: HashMap<String, Vec<String>>,
}

/// What a field's widget shows for a value. Empty for none. JSON text for an `Other`.
pub fn field_text(kind: FieldKind, value: &Value) -> String {
    match (kind, value) {
        (_, Value::Null) => String::new(),
        (FieldKind::Other, value) => serde_json::to_string_pretty(value).unwrap_or_default(),
        (FieldKind::Flags, _) => flag_set(value).join(", "),
        (_, Value::String(text)) => text.clone(),
        (_, value) => value.to_string(),
    }
}

/// Reads what was typed into a field as the JSON its component's data holds. The caller gives
/// an empty text back to the prototype, so the text here has something in it.
pub fn parse_field(field: &DataField, text: &str) -> Result<Value, String> {
    let text = text.trim();
    let number = |text: &str| text.parse::<f64>().ok().filter(|number| number.is_finite()).ok_or_else(|| format!("{text} isn't a number"));
    Ok(match field.kind {
        FieldKind::Bool => match text.to_lowercase().as_str() {
            "true" => json!(true),
            "false" => json!(false),
            _ => return Err(format!("{text} isn't true or false")),
        },
        FieldKind::Int => json!(text.parse::<i64>().map_err(|_| format!("{text} isn't a whole number"))?),
        FieldKind::Float => json!(number(text)?),
        FieldKind::Timespan => json!(number(text.strip_suffix('s').unwrap_or(text).trim())?),
        FieldKind::String | FieldKind::Prototype => json!(text),
        FieldKind::Enum => json!(name_of(field, text)?),
        FieldKind::Flags => {
            let chosen = text.split(',').map(str::trim).filter(|name| !name.is_empty()).map(|name| name_of(field, name)).collect::<Result<Vec<_>, _>>()?;
            flags_value(field, &chosen)
        }
        FieldKind::Color => json!(colour(text).ok_or_else(|| format!("{text} isn't a colour like #RRGGBB or #RRGGBBAA"))?),
        FieldKind::Vector2 => {
            let parts: Vec<&str> = text.split(|c: char| c == ',' || c.is_whitespace()).filter(|part| !part.is_empty()).collect();
            let [x, y] = parts[..] else { return Err(format!("{text} isn't two numbers like x, y")) };
            json!(format!("{},{}", number(x)?, number(y)?))
        }
        FieldKind::Entity => match text.parse::<i64>() {
            _ if text.eq_ignore_ascii_case("invalid") => json!("invalid"),
            Ok(uid) if uid >= 0 => json!(uid),
            _ => return Err(format!("{text} isn't an entity number")),
        },
        FieldKind::Other => serde_json::from_str(text).map_err(|error| format!("the text isn't valid JSON: {error}"))?,
    })
}

/// One of a field's names, as the engine writes it, for a name typed in either case. A field
/// that gives no names takes any.
fn name_of(field: &DataField, typed: &str) -> Result<String, String> {
    if field.names().is_empty() {
        return Ok(typed.to_owned());
    }
    field.names().iter().find(|name| name.eq_ignore_ascii_case(typed)).cloned().ok_or_else(|| format!("{typed} isn't one of {}", field.names().join(", ")))
}

/// #RRGGBB or #RRGGBBAA, the # optional, as the engine writes a colour: #RRGGBBAA in capitals.
pub fn colour(text: &str) -> Option<String> {
    let digits = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if !matches!(digits.len(), 6 | 8) || !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}{}", digits.to_uppercase(), if digits.len() == 6 { "FF" } else { "" }))
}

/// The names a set of flags holds: a list of them, or one text of them with commas. A text of
/// no flag ("0") holds none.
pub fn flag_set(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items.iter().map(|item| item.as_str().map_or_else(|| item.to_string(), str::to_owned)).collect(),
        Value::String(text) => text.split(',').map(str::trim).filter(|name| !name.is_empty() && *name != "0").map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}

/// These flags in the form the field is written in: a list where the game writes a list, else
/// one text with commas, and "0" for none.
pub fn flags_value(field: &DataField, chosen: &[String]) -> Value {
    if field.value.is_array() || field.prototype.is_array() {
        return json!(chosen);
    }
    if chosen.is_empty() {
        return json!("0");
    }
    json!(chosen.join(", "))
}

/// The field's flags with one of them set or cleared, in the order of its names.
pub fn with_flag(field: &DataField, flag: &str, on: bool) -> Value {
    let mut set = flag_set(&field.value);
    set.retain(|name| name != flag);
    if on {
        set.push(flag.to_owned());
    }
    let order = |name: &String| field.names().iter().position(|known| known == name).unwrap_or(usize::MAX);
    set.sort_by_key(order);
    flags_value(field, &set)
}

/// Whether two values are the same data: numbers by their worth, and a number or a flag written
/// as text the same as written plainly, as the engine reads them.
pub fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::String(text), Value::Number(number)) | (Value::Number(number), Value::String(text)) => text.trim().parse::<f64>().ok() == number.as_f64(),
        (Value::String(text), Value::Bool(flag)) | (Value::Bool(flag), Value::String(text)) => text.eq_ignore_ascii_case(&flag.to_string()),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same_value(x, y)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(key, x)| y.get(key).is_some_and(|y| same_value(x, y))),
        _ => a == b,
    }
}

#[cfg(test)]
#[path = "tests/fields.rs"]
mod tests;
