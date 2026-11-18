//! Editing the selected entity: its components, its name and its containers.

use super::*;
use crate::fields::{FieldKind, FieldsAnswer};
use crate::model::Container;

const HISTORY: &str = r#"{"canUndo":true,"canRedo":false,"revision":1,"undoLabel":null,"redoLabel":null}"#;

/// The wall's own data as the engine sends it: its placement, a battery its prototype does
/// not have, and tags its prototype has.
fn wall_data() -> Value {
    json!([{ "type": "Transform", "pos": "3.5,1.5" }, { "type": "Battery", "maxCharge": 100 }, { "type": "Tag", "tags": ["Wall", "Shiny"] }])
}

/// A session the test answers for the engine, with the scene opened in it as the engine
/// opens a map. The wall's prototype has a sprite, a light and tags, and a crowbar can be made.
pub(super) fn opened(scene: &str) -> (Session, Sender<FromEngine>, Receiver<ToEngine>) {
    let (to_engine, asked) = mpsc::channel();
    let (answers, from_engine) = mpsc::channel();
    let mut session = Session::with_channels(to_engine, from_engine, Arc::new(AtomicU64::new(0)), String::new());
    let scene: SceneJson = serde_json::from_str(scene).unwrap();
    answers.send(FromEngine::Opened { scene: Box::new(scene), list: Vec::new(), timing: EngineTiming::default(), path: None, links: Ok(Default::default()) }).ok().expect("a session to send to");
    session.pump(VIEW);
    let names = |names: &[&str]| names.iter().map(|&name| Arc::from(name)).collect();
    session.set_prototypes(vec![
        Prototype { id: "Crowbar".into(), name: "crowbar".into(), search: "crowbar crowbar".into(), components: names(&["Item", "Sprite"]), ..Default::default() },
        Prototype { id: "WallSolid".into(), name: "solid wall".into(), components: names(&["PointLight", "Sprite", "Tag"]), ..Default::default() },
    ]);
    (session, answers, asked)
}

/// A session as `opened` gives it, with the wall selected, and its data and containers sent:
/// the crowbar in its storage, an empty label slot and a full cell slot.
fn wall() -> (Session, Sender<FromEngine>, Receiver<ToEngine>) {
    let (mut session, answers, engine) = opened(&scene());
    session.selection = vec![WALL];
    session.pump(VIEW);
    assert_eq!(questions(&engine), ["details 11", "containers 11"]);
    answers.send(FromEngine::Details { uid: WALL, components: wall_data() }).ok().expect("a session to send to");
    answers.send(FromEngine::Containers { uid: WALL, containers: Ok(containers(&[CROWBAR])) }).ok().expect("a session to send to");
    session.pump(VIEW);
    (session, answers, engine)
}

/// The wall's containers, with these in its storage.
fn containers(stored: &[i32]) -> Vec<Container> {
    let container = |id: &str, kind: &str, entities: &[i32], can_insert: bool| Container { id: id.into(), kind: kind.into(), entities: entities.to_vec(), can_insert };
    vec![container("entity_storage", "Container", stored, true), container("paper_label", "ContainerSlot", &[], true), container("cell_slot", "ContainerSlot", &[90], false)]
}

/// The questions asked since the last call, and nothing else.
fn questions(from: &Receiver<ToEngine>) -> Vec<String> {
    from.try_iter()
        .filter_map(|command| match command {
            ToEngine::Details { uid } => Some(format!("details {uid}")),
            ToEngine::Containers { uid } => Some(format!("containers {uid}")),
            _ => None,
        })
        .collect()
}

/// The one edit asked since the last call: its name, its number, and its one op. Questions
/// asked beside it are dropped.
#[track_caller]
pub(super) fn one_edit(from: &Receiver<ToEngine>) -> (&'static str, u64, Value) {
    let edits: Vec<(&'static str, Option<u64>, u64, Vec<Value>)> =
        from.try_iter().filter_map(|command| match command { ToEngine::Edit { ops, label, stroke, seq, .. } => Some((label, stroke, seq, ops)), _ => None }).collect();
    assert_eq!(edits.len(), 1, "{edits:?}");
    let (label, stroke, seq, ops) = edits.into_iter().next().unwrap();
    // One edit with no stroke is one step to undo.
    assert_eq!((stroke, ops.len()), (None, 1));
    (label, seq, ops.into_iter().next().unwrap())
}

/// The list of components a `components` op sends.
fn sent_list(op: &Value) -> Value {
    assert_eq!((op["kind"].as_str(), op["uid"].as_i64()), (Some("components"), Some(WALL as i64)));
    serde_json::from_str(op["components"].as_str().unwrap()).unwrap()
}

/// What the engine says a command changed.
pub(super) fn delta(entities: &[String], removed: &[i32], grids: &[String], created: &[i32], details: &[(i32, Value)]) -> Box<Delta> {
    let details: Vec<Value> = details.iter().map(|(uid, components)| json!({ "uid": uid, "components": components.to_string() })).collect();
    let json = format!(
        r#"{{"tiles":[],"entities":[{}],"removed":{removed:?},"gridsChanged":[{}],"history":{HISTORY},"created":{created:?},"details":{},"linked":[]}}"#,
        entities.join(","),
        grids.join(","),
        Value::Array(details)
    );
    Box::new(serde_json::from_str(&json).unwrap())
}

/// The engine answers a command with what it changed.
pub(super) fn answer(session: &mut Session, answers: &Sender<FromEngine>, seq: u64, what: &'static str, delta: Box<Delta>) {
    answers.send(FromEngine::Changed { list: None, delta: Some(delta), what, seq, asked: Instant::now(), timing: Timing::default() }).ok().expect("a session to send to");
    session.pump(VIEW);
}

/// The engine refuses a command.
pub(super) fn refuse(session: &mut Session, answers: &Sender<FromEngine>, seq: u64, what: &'static str, error: &str) {
    answers.send(FromEngine::Refused { seq, what, error: error.into() }).ok().expect("a session to send to");
    session.pump(VIEW);
}

/// An entity as the engine describes one.
pub(super) fn entity(uid: i32, prototype: &str, parent: i32, contained: bool, name: Option<&str>) -> String {
    let name = name.map_or("null".to_owned(), |name| format!("\"{name}\""));
    format!(r#"{{"uid":{uid},"yamlUid":0,"prototype":"{prototype}","parent":{parent},"x":0,"y":0,"rotation":0,"anchored":false,"contained":{contained},"name":{name}}}"#)
}

fn row(session: &Session, name: &str) -> ComponentRow {
    session.component_rows().into_iter().find(|row| row.name == name).expect("a row of that component")
}

#[test]
fn the_rows_are_the_entity_s_own_data_with_what_each_is() {
    let (session, _answers, _engine) = wall();
    let rows: Vec<(String, bool, bool)> = session.component_rows().into_iter().map(|row| (row.name, row.from_prototype, row.reserved)).collect();
    assert_eq!(rows, [("Transform".into(), false, true), ("Battery".into(), false, false), ("Tag".into(), true, false)]);
    assert_eq!(row(&session, "Battery").text, "{\n  \"maxCharge\": 100\n}");
    assert!(!session.entity_busy() && session.entity_error().is_none());
}

#[test]
fn apply_sends_the_whole_list_with_the_edited_component_as_one_step() {
    let (mut session, answers, engine) = wall();
    session.set_component_text("Battery", "{ \"maxCharge\": 250, \"startingCharge\": 50 }".into());
    assert!(row(&session, "Battery").edited);

    session.apply_component("Battery");
    let (label, seq, op) = one_edit(&engine);
    assert_eq!(label, "Edit component");
    // The others go as the engine sent them, in their places.
    let mut wanted = wall_data();
    wanted[1] = json!({ "type": "Battery", "maxCharge": 250, "startingCharge": 50 });
    assert_eq!(sent_list(&op), wanted);
    assert!(session.entity_busy());

    // The answer brings the fresh data, and the text is the engine's again.
    answer(&mut session, &answers, seq, "Edit component", delta(&[], &[], &[], &[], &[(WALL, wanted)]));
    let battery = row(&session, "Battery");
    assert_eq!((battery.edited, battery.text.contains("250")), (false, true));
    assert!(!session.entity_busy());
}

#[test]
fn text_that_is_not_one_object_is_refused_before_anything_is_sent() {
    let (mut session, _answers, engine) = wall();
    session.set_component_text("Battery", "[1, 2]".into());
    session.apply_component("Battery");
    assert_eq!(session.entity_error(), Some("Battery not applied: the text must be a single JSON object in braces"));
    session.set_component_text("Battery", "{ \"maxCharge\": ".into());
    session.apply_component("Battery");
    assert!(session.entity_error().unwrap().starts_with("Battery not applied: the text isn't valid JSON: "));
    assert!(asked(&engine).is_empty());
    // The text stays for the user to mend.
    assert_eq!(row(&session, "Battery").text, "{ \"maxCharge\": ");
}

#[test]
fn a_refusal_is_shown_whole_and_leaves_everything_as_it_was() {
    let (mut session, answers, engine) = wall();
    session.set_component_text("Battery", "{ \"maxCharge\": \"lots\" }".into());
    session.apply_component("Battery");
    let (_, seq, _) = one_edit(&engine);

    let error = "Could not read Battery: \"lots\" is no float.\n   at the serializer";
    refuse(&mut session, &answers, seq, "Edit component", error);
    assert_eq!(session.entity_error(), Some(error));
    assert_eq!(session.status_line(), "Edit component: the engine refused: Could not read Battery: \"lots\" is no float.");
    assert_eq!(session.details, Some((WALL, wall_data())));
    // The text is kept, and a next change can be sent.
    assert!(row(&session, "Battery").edited && !session.entity_busy());
    session.revert_component("Battery");
    assert!(!row(&session, "Battery").edited);
}

#[test]
fn no_second_change_goes_while_the_first_waits() {
    let (mut session, answers, engine) = wall();
    session.remove_component("Battery");
    let (_, seq, _) = one_edit(&engine);
    session.remove_component("Tag");
    session.rename_entity("Bob");
    assert!(asked(&engine).is_empty());
    assert_eq!(session.status_line(), "Waiting for the engine to finish this entity's last change");

    // An answer to something sent before it does not end the wait.
    answer(&mut session, &answers, seq - 1, "Move", delta(&[], &[], &[], &[], &[]));
    assert!(session.entity_busy());
    answer(&mut session, &answers, seq, "Remove component", delta(&[], &[], &[], &[], &[]));
    assert!(!session.entity_busy());
}

#[test]
fn a_component_is_added_and_one_the_prototype_has_waits_as_a_draft() {
    let (mut session, answers, engine) = wall();
    session.add_component(" Igniter ");
    let (label, seq, op) = one_edit(&engine);
    assert_eq!(label, "Add component");
    let mut wanted = wall_data();
    wanted.as_array_mut().unwrap().push(json!({ "type": "Igniter" }));
    assert_eq!(sent_list(&op), wanted);
    answer(&mut session, &answers, seq, "Add component", delta(&[], &[], &[], &[], &[(WALL, wanted.clone())]));

    // The light comes from the prototype. With no fields it would change nothing, so it waits.
    session.add_component("PointLight");
    assert!(asked(&engine).is_empty());
    let light = row(&session, "PointLight");
    assert_eq!((light.draft, light.from_prototype, light.text.as_str()), (true, true, "{}"));
    session.set_component_text("PointLight", "{ \"radius\": 4 }".into());
    session.apply_component("PointLight");
    let (label, seq, op) = one_edit(&engine);
    assert_eq!(label, "Add component");
    assert_eq!(sent_list(&op).as_array().unwrap().last(), Some(&json!({ "type": "PointLight", "radius": 4 })));
    let mut lit = wanted.clone();
    lit.as_array_mut().unwrap().push(json!({ "type": "PointLight", "radius": 4 }));
    answer(&mut session, &answers, seq, "Add component", delta(&[], &[], &[], &[], &[(WALL, lit)]));
    assert!(!row(&session, "PointLight").draft);

    // One it has, one the placement owns, and no name are refused.
    session.add_component("Battery");
    assert_eq!(session.entity_error(), Some("Battery not added: the entity already has it"));
    session.add_component("Transform");
    assert_eq!(session.entity_error(), Some("Transform not added: moving and renaming change it"));
    session.add_component("  ");
    assert_eq!(session.entity_error(), Some("No component added: type its name"));
    assert!(asked(&engine).is_empty());

    // A draft that is dropped sends nothing.
    session.add_component("Sprite");
    session.remove_component("Sprite");
    assert!(asked(&engine).is_empty() && session.component_rows().iter().all(|row| row.name != "Sprite"));
}

#[test]
fn a_component_is_removed_or_given_back_to_the_prototype() {
    let (mut session, answers, engine) = wall();
    session.remove_component("Battery");
    let (label, seq, op) = one_edit(&engine);
    assert_eq!(label, "Remove component");
    assert_eq!(sent_list(&op), json!([{ "type": "Transform", "pos": "3.5,1.5" }, { "type": "Tag", "tags": ["Wall", "Shiny"] }]));
    answer(&mut session, &answers, seq, "Remove component", delta(&[], &[], &[], &[], &[]));

    // The prototype has tags, so the wall's own go and the prototype's are back.
    session.remove_component("Tag");
    let (label, _, op) = one_edit(&engine);
    assert_eq!(label, "Reset component");
    assert_eq!(sent_list(&op), json!([{ "type": "Transform", "pos": "3.5,1.5" }, { "type": "Battery", "maxCharge": 100 }]));
}

/// The fields of the wall's components as the engine tells them. The battery is not the
/// prototype's, so its prototype values are a new battery's. The light's mask names a kind of
/// prototype whose ids come with the answer.
fn wall_fields() -> FieldsAnswer {
    serde_json::from_value(json!({
        "components": [
            { "name": "Battery", "fields": [
                { "tag": "maxCharge", "kind": "float", "type": "Single", "value": 100, "prototype": 0, "readOnly": false },
                { "tag": "startingCharge", "kind": "float", "type": "Single", "value": 0, "prototype": 0, "readOnly": false },
            ] },
            { "name": "PointLight", "fields": [
                { "tag": "enabled", "kind": "bool", "type": "Boolean", "value": true, "prototype": true, "readOnly": false },
                { "tag": "radius", "kind": "float", "type": "Single", "value": 6, "prototype": 6, "readOnly": false },
                { "tag": "color", "kind": "color", "type": "Color", "value": "#FFE2C8FF", "prototype": "#FFE2C8FF", "readOnly": false },
                { "tag": "lightMask", "kind": "prototype", "type": "ProtoId<LightMaskPrototype>?", "value": null, "prototype": null, "readOnly": false, "prototypeKind": "lightMask" },
                { "tag": "init", "kind": "other", "type": "Dictionary<Enum, Object>", "value": null, "prototype": null, "readOnly": true },
            ] },
            { "name": "Physics", "fields": [
                { "tag": "bodyType", "kind": "flags", "type": "BodyType", "value": "Static", "prototype": "Static", "readOnly": false, "names": ["Kinematic", "Static", "Dynamic"] },
            ] },
            { "name": "Tag", "fields": [
                { "tag": "tags", "kind": "other", "type": "HashSet<ProtoId<TagPrototype>>", "value": ["Wall", "Shiny"], "prototype": ["Wall"], "readOnly": false },
            ] },
        ],
        "ids": { "lightMask": ["Cone", "Round"] },
    }))
    .unwrap()
}

/// The wall as `wall` gives it, with its fields told.
fn formed() -> (Session, Sender<FromEngine>, Receiver<ToEngine>) {
    let (mut session, answers, engine) = wall();
    answers.send(FromEngine::Fields { uid: WALL, answer: Ok(wall_fields()) }).ok().expect("a session to send to");
    session.pump(VIEW);
    (session, answers, engine)
}

/// How many times the fields were asked for since the last call.
fn fields_asked(from: &Receiver<ToEngine>) -> usize {
    from.try_iter().filter(|command| matches!(command, ToEngine::Fields { uid: WALL })).count()
}

fn field(session: &Session, name: &str, tag: &str) -> FieldRow {
    let entry = session.component_entries().into_iter().find(|entry| entry.row.name == name).expect("a component of that name");
    entry.fields.expect("a form").into_iter().find(|field| field.tag == tag).expect("a field of that tag")
}

#[test]
fn every_component_has_a_form_once_the_fields_are_told() {
    let (mut session, answers, engine) = wall();
    // Asked for with the data, and the text rows stand until the fields come.
    assert_eq!(fields_asked(&engine), 0);
    let entries = session.component_entries();
    assert!(entries.iter().all(|entry| entry.fields.is_none()) && entries.len() == 3);
    answers.send(FromEngine::Fields { uid: WALL, answer: Ok(wall_fields()) }).ok().expect("a session to send to");
    session.pump(VIEW);

    // Sorted by name: the entity's own and the prototype's, with Transform's text alone.
    let entries: Vec<(String, bool, bool)> = session.component_entries().into_iter().map(|entry| (entry.row.name, entry.own, entry.fields.is_some())).collect();
    let wanted = [("Battery", true, true), ("Physics", false, true), ("PointLight", false, true), ("Tag", true, true), ("Transform", true, false)];
    assert_eq!(entries, wanted.map(|(name, own, form)| (name.to_owned(), own, form)));
    let charge = field(&session, "Battery", "maxCharge");
    assert_eq!((charge.kind, charge.text.as_str(), charge.own), (FieldKind::Float, "100", true));
    assert!(!field(&session, "Battery", "startingCharge").own);
    assert_eq!(field(&session, "Tag", "tags").text, "[\n  \"Wall\",\n  \"Shiny\"\n]");
    assert_eq!(field(&session, "Physics", "bodyType").names, ["Kinematic", "Static", "Dynamic"]);

    // An engine that would not tell leaves the text rows, with why.
    answers.send(FromEngine::Fields { uid: WALL, answer: Err("Unknown engine command 'component.fields'.".into()) }).ok().expect("a session to send to");
    session.pump(VIEW);
    assert_eq!(session.component_fields_error(), Some("Unknown engine command 'component.fields'."));
    assert!(session.component_entries().iter().all(|entry| entry.fields.is_none()));
}

#[test]
fn a_field_change_sends_the_component_s_own_data_with_it() {
    let (mut session, answers, engine) = formed();
    session.set_component_field("Battery", "startingCharge", " 75.5 ");
    let (label, seq, op) = one_edit(&engine);
    assert_eq!(label, "Edit component");
    // The others go as the engine sent them, and the battery keeps its charge.
    let mut wanted = wall_data();
    wanted[1] = json!({ "type": "Battery", "maxCharge": 100, "startingCharge": 75.5 });
    assert_eq!(sent_list(&op), wanted);
    // The answer brings the new data, and the fields are asked for again.
    answer(&mut session, &answers, seq, "Edit component", delta(&[], &[], &[], &[], &[(WALL, wanted.clone())]));
    assert_eq!(fields_asked(&engine), 1);

    // A field of the prototype's light makes the component's own data. A checkbox sends its value.
    session.set_component_field("PointLight", "color", "#ff8800");
    let (_, seq, op) = one_edit(&engine);
    assert_eq!(sent_list(&op)[3], json!({ "type": "PointLight", "color": "#FF8800FF" }));
    answer(&mut session, &answers, seq, "Edit component", delta(&[], &[], &[], &[], &[]));
    session.set_component_value("PointLight", "enabled", json!(false));
    let (_, seq, op) = one_edit(&engine);
    assert_eq!(sent_list(&op)[3], json!({ "type": "PointLight", "enabled": false }));
    answer(&mut session, &answers, seq, "Edit component", delta(&[], &[], &[], &[], &[]));

    // A flag set in the form the game writes it in.
    session.set_component_flag("Physics", "bodyType", "Dynamic", true);
    let (_, _, op) = one_edit(&engine);
    assert_eq!(sent_list(&op)[3], json!({ "type": "Physics", "bodyType": "Static, Dynamic" }));
}

#[test]
fn a_field_set_to_the_prototype_s_value_is_dropped_from_the_data() {
    let (mut session, answers, engine) = formed();
    // The battery's charge back to a new battery's: the battery stays, with no data of its own.
    session.set_component_field("Battery", "maxCharge", "0");
    let (_, seq, op) = one_edit(&engine);
    assert_eq!(sent_list(&op)[1], json!({ "type": "Battery" }));
    answer(&mut session, &answers, seq, "Edit component", delta(&[], &[], &[], &[], &[]));

    // The prototype's value where the entity has none of its own, or the value it has, sends nothing.
    session.set_component_field("PointLight", "radius", "6.0");
    session.set_component_field("Battery", "maxCharge", "100");
    session.set_component_value("Tag", "tags", json!(["Wall", "Shiny"]));
    assert!(asked(&engine).is_empty() && session.entity_error().is_none());
}

#[test]
fn a_field_is_reset_to_the_prototype() {
    let (mut session, answers, engine) = formed();
    session.reset_component_field("Tag", "tags");
    let (label, seq, op) = one_edit(&engine);
    assert_eq!(label, "Reset field");
    assert_eq!(sent_list(&op)[2], json!({ "type": "Tag" }));
    answer(&mut session, &answers, seq, "Reset field", delta(&[], &[], &[], &[], &[]));

    // Nothing typed is a reset. A field with nothing of its own has nothing to reset.
    session.set_component_field("Battery", "maxCharge", "  ");
    let (label, seq, op) = one_edit(&engine);
    assert_eq!((label, &sent_list(&op)[1]), ("Reset field", &json!({ "type": "Battery" })));
    answer(&mut session, &answers, seq, "Reset field", delta(&[], &[], &[], &[], &[]));
    session.reset_component_field("Battery", "startingCharge");
    session.reset_component_field("PointLight", "radius");
    assert!(asked(&engine).is_empty());
}

#[test]
fn what_cannot_be_set_is_refused_before_anything_is_sent() {
    let (mut session, answers, engine) = formed();
    session.set_component_field("Battery", "maxCharge", "full");
    assert_eq!(session.entity_error(), Some("Battery maxCharge not set: full isn't a number"));
    session.set_component_field("PointLight", "lightMask", "Square");
    assert_eq!(session.entity_error(), Some("PointLight lightMask not set: the game has no lightMask prototype Square"));
    session.set_component_value("PointLight", "init", json!({}));
    assert_eq!(session.entity_error(), Some("PointLight init not set: the game never writes it"));
    session.set_component_field("Tag", "tags", "[\"Wall\", ");
    assert!(session.entity_error().unwrap().starts_with("Tag tags not set: the text isn't valid JSON: "));
    assert!(asked(&engine).is_empty());

    // An id the game has is sent.
    session.set_component_field("PointLight", "lightMask", "Cone");
    let (_, seq, op) = one_edit(&engine);
    assert_eq!(sent_list(&op)[3], json!({ "type": "PointLight", "lightMask": "Cone" }));
    answer(&mut session, &answers, seq, "Edit component", delta(&[], &[], &[], &[], &[]));

    // The ids are kept for the next entity, as the engine tells a kind once.
    session.selection = vec![CHAIR];
    session.pump(VIEW);
    answers.send(FromEngine::Fields { uid: CHAIR, answer: Ok(serde_json::from_value(json!({ "components": [{ "name": "PointLight", "fields": [
        { "tag": "lightMask", "kind": "prototype", "type": "", "value": null, "prototype": null, "readOnly": false, "prototypeKind": "lightMask" }] }] })).unwrap()) }).ok().expect("a session to send to");
    answers.send(FromEngine::Details { uid: CHAIR, components: json!([]) }).ok().expect("a session to send to");
    session.pump(VIEW);
    session.set_component_field("PointLight", "lightMask", "Square");
    assert_eq!(session.entity_error(), Some("PointLight lightMask not set: the game has no lightMask prototype Square"));
}

#[test]
fn a_rename_is_one_step_and_the_data_is_asked_for_again() {
    let (mut session, answers, engine) = wall();
    session.rename_entity(" Bob ");
    let (label, seq, op) = one_edit(&engine);
    assert_eq!((label, op), ("Rename", json!({ "kind": "rename", "uid": WALL, "name": "Bob" })));
    answer(&mut session, &answers, seq, "Rename", delta(&[entity(WALL, "WallSolid", GRID, false, Some("Bob"))], &[], &[], &[], &[]));
    assert_eq!(questions(&engine), ["details 11"]);
    assert_eq!(session.doc.as_ref().unwrap().entities[&WALL].name.as_deref(), Some("Bob"));
    // Until the data has come, no change goes.
    assert!(session.entity_busy());
    answers.send(FromEngine::Details { uid: WALL, components: wall_data() }).ok().expect("a session to send to");
    session.pump(VIEW);
    assert!(!session.entity_busy());
}

#[test]
fn the_names_offered_are_those_of_the_prototypes() {
    let (mut session, _answers, _engine) = wall();
    let names = |names: Vec<Arc<str>>| names.iter().map(|name| name.to_string()).collect::<Vec<_>>();
    assert_eq!(names(session.component_names_matching("", 10)), ["Item", "PointLight", "Sprite", "Tag"]);
    assert_eq!(names(session.component_names_matching("LIGHT", 10)), ["PointLight"]);
    assert_eq!(names(session.component_names_matching("", 2)), ["Item", "PointLight"]);
}

#[test]
fn the_containers_are_told_for_the_one_entity_selected() {
    let (mut session, answers, engine) = wall();
    assert_eq!(session.containers(), Some(&Ok(containers(&[CROWBAR]))));

    // Another entity starts afresh. An engine that would not tell is shown why.
    session.selection = vec![CHAIR];
    session.pump(VIEW);
    assert_eq!(questions(&engine), ["details 10", "containers 10"]);
    assert_eq!((session.containers(), session.details.as_ref()), (None, None));
    answers.send(FromEngine::Containers { uid: CHAIR, containers: Err("Unknown engine command 'containers'.".into()) }).ok().expect("a session to send to");
    // An answer for one no longer selected is dropped.
    answers.send(FromEngine::Containers { uid: WALL, containers: Ok(Vec::new()) }).ok().expect("a session to send to");
    session.pump(VIEW);
    assert_eq!(session.containers(), Some(&Err("Unknown engine command 'containers'.".into())));

    session.selection = vec![CHAIR, WALL];
    session.pump(VIEW);
    assert!(session.containers().is_none() && questions(&engine).is_empty());
}

#[test]
fn a_thing_is_added_to_a_container_and_the_list_is_asked_again() {
    let (mut session, answers, engine) = wall();
    session.contain("entity_storage", "Crowbar");
    let (label, seq, op) = one_edit(&engine);
    assert_eq!((label, op), ("Add to container", json!({ "kind": "contain", "parent": WALL, "container": "entity_storage", "prototype": "Crowbar" })));

    answer(&mut session, &answers, seq, "Add to container", delta(&[entity(60, "Crowbar", WALL, true, None)], &[], &[], &[60], &[(WALL, wall_data())]));
    assert_eq!(questions(&engine), ["containers 11"]);
    assert_eq!(uids(&session.doc.as_ref().unwrap().contents(WALL)), [CROWBAR, 60]);
    answers.send(FromEngine::Containers { uid: WALL, containers: Ok(containers(&[CROWBAR, 60])) }).ok().expect("a session to send to");
    session.pump(VIEW);
    assert_eq!(session.containers().unwrap().as_ref().unwrap()[0].entities, [CROWBAR, 60]);

    // A full slot and a container it does not have are refused before anything is sent.
    session.contain("cell_slot", "Crowbar");
    assert_eq!(session.entity_error(), Some("Not added: cell_slot is full"));
    session.contain("pai_slot", "Crowbar");
    assert_eq!(session.entity_error(), Some("Not added: the entity has no container pai_slot"));
    assert!(asked(&engine).is_empty());
}

#[test]
fn a_held_thing_is_removed_and_a_refusal_leaves_the_list() {
    let (mut session, answers, engine) = wall();
    session.uncontain(CROWBAR);
    let (label, seq, op) = one_edit(&engine);
    assert_eq!((label, op), ("Remove from container", json!({ "kind": "uncontain", "uid": CROWBAR })));
    answer(&mut session, &answers, seq, "Remove from container", delta(&[], &[CROWBAR], &[], &[], &[(WALL, wall_data())]));
    assert_eq!(questions(&engine), ["containers 11"]);
    assert!(session.doc.as_ref().unwrap().contents(WALL).is_empty());

    // The engine refuses: its words are shown, and nothing is asked again.
    session.contain("paper_label", "Paper");
    let (_, seq, _) = one_edit(&engine);
    refuse(&mut session, &answers, seq, "Add to container", "Unknown entity prototype 'Paper'.");
    assert_eq!(session.entity_error(), Some("Unknown entity prototype 'Paper'."));
    assert!(questions(&engine).is_empty());
    assert_eq!(session.containers(), Some(&Ok(containers(&[CROWBAR]))));
}

#[test]
fn an_undo_asks_for_the_data_and_the_containers_again() {
    let (mut session, answers, engine) = wall();
    session.undo();
    let seq = engine.try_iter().find_map(|command| match command { ToEngine::Undo { seq, .. } => Some(seq), _ => None }).unwrap();
    answer(&mut session, &answers, seq, "Undo", delta(&[], &[], &[], &[], &[]));
    assert_eq!(questions(&engine), ["details 11", "containers 11"]);

    // A move of the wall itself, as a drag sends many of, asks nothing.
    answer(&mut session, &answers, seq + 1, "Move", delta(&[entity(WALL, "WallSolid", GRID, false, None)], &[], &[], &[], &[]));
    assert!(questions(&engine).is_empty());
}

fn uids(entities: &[&Entity]) -> Vec<i32> {
    entities.iter().map(|entity| entity.uid).collect()
}
