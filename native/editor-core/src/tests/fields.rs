use super::*;

/// A field of a kind, with these names, now and in the prototype.
fn field(kind: FieldKind, names: &[&str], value: Value, prototype: Value) -> DataField {
    DataField {
        tag: "field".into(),
        kind,
        type_name: String::new(),
        value,
        prototype,
        read_only: false,
        names: (!names.is_empty()).then(|| names.iter().map(|&name| name.to_owned()).collect()),
        prototype_kind: None,
    }
}

fn of(kind: FieldKind) -> DataField {
    field(kind, &[], Value::Null, Value::Null)
}

#[test]
fn the_engine_s_answer_is_read_with_each_kind() {
    let answer: FieldsAnswer = serde_json::from_str(
        r#"{"components":[{"name":"PointLight","fields":[
            {"tag":"radius","kind":"float","type":"Single","value":6,"prototype":6,"readOnly":false,"names":null,"prototypeKind":null},
            {"tag":"lightMask","kind":"prototype","type":"ProtoId<LightMaskPrototype>?","value":null,"prototype":null,"readOnly":false,"prototypeKind":"lightMask"},
            {"tag":"state","kind":"enum","type":"DoorState","value":"Closed","prototype":"Closed","readOnly":false,"names":["Closed","Open"]},
            {"tag":"init","kind":"somethingnew","type":"","value":null,"prototype":null,"readOnly":true}]}],
          "ids":{"lightMask":["Cone","Round"]}}"#,
    )
    .unwrap();
    let fields = &answer.components[0].fields;
    let kinds: Vec<FieldKind> = fields.iter().map(|field| field.kind).collect();
    assert_eq!(kinds, [FieldKind::Float, FieldKind::Prototype, FieldKind::Enum, FieldKind::Other]);
    assert_eq!((fields[1].prototype_kind.as_deref(), fields[2].names(), fields[3].read_only), (Some("lightMask"), &["Closed".to_owned(), "Open".to_owned()][..], true));
    assert_eq!(answer.ids["lightMask"], ["Cone", "Round"]);
}

#[test]
fn bools_ints_floats_and_times_read_as_their_kind() {
    assert_eq!(parse_field(&of(FieldKind::Bool), " False "), Ok(json!(false)));
    assert!(parse_field(&of(FieldKind::Bool), "no").is_err());
    assert_eq!(parse_field(&of(FieldKind::Int), "-12"), Ok(json!(-12)));
    assert_eq!(parse_field(&of(FieldKind::Int), "1.5"), Err("1.5 isn't a whole number".into()));
    assert_eq!(parse_field(&of(FieldKind::Float), " 75.5 "), Ok(json!(75.5)));
    assert_eq!(parse_field(&of(FieldKind::Float), "full"), Err("full isn't a number".into()));
    assert!(parse_field(&of(FieldKind::Float), "NaN").is_err());
    assert_eq!(parse_field(&of(FieldKind::Timespan), "2.5s"), Ok(json!(2.5)));
    assert_eq!(parse_field(&of(FieldKind::Timespan), "0.4"), Ok(json!(0.4)));
}

#[test]
fn texts_ids_and_entities_read_as_their_kind() {
    assert_eq!(parse_field(&of(FieldKind::String), "  closed_unlit "), Ok(json!("closed_unlit")));
    assert_eq!(parse_field(&of(FieldKind::Prototype), "Cone"), Ok(json!("Cone")));
    assert_eq!(parse_field(&of(FieldKind::Entity), "Invalid"), Ok(json!("invalid")));
    assert_eq!(parse_field(&of(FieldKind::Entity), "42"), Ok(json!(42)));
    assert!(parse_field(&of(FieldKind::Entity), "-1").is_err() && parse_field(&of(FieldKind::Entity), "wall").is_err());
}

#[test]
fn an_enum_takes_one_of_its_names_in_either_case() {
    let state = field(FieldKind::Enum, &["Closed", "Open"], json!("Closed"), json!("Closed"));
    assert_eq!(parse_field(&state, "open"), Ok(json!("Open")));
    assert_eq!(parse_field(&state, "Ajar"), Err("Ajar isn't one of Closed, Open".into()));
    // An enum the engine gave no names of takes what is typed.
    assert_eq!(parse_field(&of(FieldKind::Enum), "Anything"), Ok(json!("Anything")));
}

#[test]
fn flags_keep_the_form_the_game_writes_them_in() {
    // A flag enum is one text with commas. A field of the flag serializer is a list.
    let body = field(FieldKind::Flags, &["Kinematic", "Static", "Dynamic"], json!("Static"), json!("Static"));
    assert_eq!(parse_field(&body, "static, dynamic"), Ok(json!("Static, Dynamic")));
    assert!(parse_field(&body, "Static, Floating").is_err());
    assert_eq!(with_flag(&body, "Kinematic", true), json!("Kinematic, Static"));
    assert_eq!(with_flag(&body, "Static", false), json!("0"));
    let layers = field(FieldKind::Flags, &["Opaque", "Impassable", "MidImpassable"], json!(["MidImpassable"]), json!([]));
    assert_eq!(with_flag(&layers, "Opaque", true), json!(["Opaque", "MidImpassable"]));
    assert_eq!(with_flag(&layers, "MidImpassable", false), json!([]));
    assert_eq!(field_text(FieldKind::Flags, &json!("A, B")), "A, B");
    assert_eq!(field_text(FieldKind::Flags, &json!(["A", "B"])), "A, B");
    assert!(flag_set(&json!("0")).is_empty());
}

#[test]
fn colours_and_vectors_are_written_as_the_engine_writes_them() {
    assert_eq!(parse_field(&of(FieldKind::Color), "ff8800"), Ok(json!("#FF8800FF")));
    assert_eq!(parse_field(&of(FieldKind::Color), "#0055ccAA"), Ok(json!("#0055CCAA")));
    assert!(parse_field(&of(FieldKind::Color), "#12345").is_err() && parse_field(&of(FieldKind::Color), "#GGGGGG").is_err());
    assert_eq!(parse_field(&of(FieldKind::Vector2), "0, -0.5"), Ok(json!("0,-0.5")));
    assert_eq!(parse_field(&of(FieldKind::Vector2), "1.5 2"), Ok(json!("1.5,2")));
    assert!(parse_field(&of(FieldKind::Vector2), "1").is_err() && parse_field(&of(FieldKind::Vector2), "1,x").is_err());
}

#[test]
fn other_fields_are_json_text() {
    let sound = json!({ "_ss14Tag": "!type:SoundPathSpecifier", "path": "/Audio/door.ogg" });
    let text = field_text(FieldKind::Other, &sound);
    assert_eq!(parse_field(&of(FieldKind::Other), &text), Ok(sound));
    assert_eq!(parse_field(&of(FieldKind::Other), "[1, 2]"), Ok(json!([1, 2])));
    assert!(parse_field(&of(FieldKind::Other), "{ \"path\": ").unwrap_err().starts_with("the text isn't valid JSON: "));
}

#[test]
fn a_field_s_text_is_its_value() {
    assert_eq!(field_text(FieldKind::Float, &json!(2.5)), "2.5");
    assert_eq!(field_text(FieldKind::String, &json!("open")), "open");
    assert_eq!(field_text(FieldKind::Bool, &json!(true)), "true");
    assert_eq!(field_text(FieldKind::String, &Value::Null), "");
}

#[test]
fn values_are_the_same_as_the_engine_reads_them() {
    assert!(same_value(&json!(6), &json!(6.0)));
    assert!(same_value(&json!("5"), &json!(5)));
    assert!(same_value(&json!("true"), &json!(true)));
    assert!(same_value(&json!({ "a": [1, "x"] }), &json!({ "a": [1.0, "x"] })));
    assert!(!same_value(&json!(6), &json!(6.5)));
    assert!(!same_value(&json!({ "a": 1 }), &json!({ "a": 1, "b": 2 })));
    assert!(!same_value(&json!(null), &json!("")));
}
