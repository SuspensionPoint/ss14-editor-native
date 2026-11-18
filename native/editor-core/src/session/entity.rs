//! Editing the one selected entity: its component data, its name and its containers.
//!
//! The engine holds the data. The session keeps the text of each component as the user edits
//! it, the components added in the panel and not applied yet, the engine's last refusal, the
//! data fields of each component for its form (`component.fields`, read in `crate::fields`),
//! and the entity's containers as the engine last told them. Each change is one edit, and so
//! one step to undo. A `components` edit sets the entity's whole list, so it is built from the
//! data the engine last sent, and no second change is sent until the first is answered. A
//! field set in a form changes the component's own data: set back to the prototype's value it
//! is dropped from it.

use std::collections::BTreeSet;

use serde_json::Map;

use super::*;
use crate::fields::{field_text, parse_field, same_value, with_flag, ComponentFields, DataField, FieldKind};
use crate::model::Container;

/// Components the placement and the name own. A `components` edit leaves them alone.
pub const RESERVED_COMPONENTS: [&str; 2] = ["Transform", "MetaData"];

/// What the session keeps of the entity being edited.
#[derive(Default)]
pub(super) struct Inspecting {
    /// The entity all this is for. Selecting another starts it afresh.
    uid: Option<i32>,
    /// Component text as the user edited it and has not applied, by component.
    texts: HashMap<String, String>,
    /// Components added in the panel that wait for fields and Apply.
    drafts: Vec<String>,
    /// Why the last change was not made, whole: the text could not be read, or the engine refused.
    error: Option<String>,
    /// The last change sent, until the engine answers it.
    waiting: Option<Waiting>,
    /// The component data was asked for again and has not come yet.
    refreshing: bool,
    /// The entity's containers as the engine last told them, or why it would not.
    containers: Option<Result<Vec<Container>, String>>,
    /// Every component name the prototypes have, sorted, once first asked for.
    names: Vec<Arc<str>>,
    /// The data fields of the entity's components as the engine last told them, or why it would not.
    fields: Option<Result<Vec<ComponentFields>, String>>,
    /// The ids of each prototype kind a field names, as the engine told them with the fields.
    /// Kept for every entity after: the engine tells a kind once.
    ids: HashMap<String, HashSet<String>>,
}

struct Waiting {
    seq: u64,
    /// The component whose text the change applied.
    component: Option<String>,
}

/// One component of the selected entity, as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentRow {
    pub name: String,
    /// What the user typed, or else the entity's own data as JSON, without its type.
    pub text: String,
    /// The text differs from what the engine sent and is not applied.
    pub edited: bool,
    /// Added in the panel and not applied yet.
    pub draft: bool,
    /// Its prototype has it, so taking the entity's data away puts back the prototype's.
    pub from_prototype: bool,
    /// Transform or MetaData, which moving and renaming change. Not edited as text.
    pub reserved: bool,
}

/// One data field of a component's form, as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldRow {
    pub tag: String,
    pub kind: FieldKind,
    pub type_name: String,
    /// What the widget shows for the value now: its text, or JSON text for an `Other`.
    pub text: String,
    pub value: Value,
    pub prototype: Value,
    /// The entity's own data sets it, apart from the prototype. The mark and the reset.
    pub own: bool,
    pub read_only: bool,
    /// An enum's or a flag set's names.
    pub names: Vec<String>,
    pub prototype_kind: Option<String>,
}

/// One component of the selected entity in the panel: its text row, and its form once the
/// engine has told the fields.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentEntry {
    pub row: ComponentRow,
    /// It has data of its own, apart from the prototype.
    pub own: bool,
    pub fields: Option<Vec<FieldRow>>,
}

impl Session {
    /// The selected entity's uid, if exactly one is selected.
    fn inspected(&self) -> Option<i32> {
        match self.selection[..] {
            [uid] => Some(uid),
            _ => None,
        }
    }

    /// The selection changed: what was kept for the entity before is dropped, and for one
    /// entity its component data and containers are asked for.
    pub(super) fn inspect(&mut self, uid: Option<i32>) {
        let names = std::mem::take(&mut self.inspecting.names);
        let ids = std::mem::take(&mut self.inspecting.ids);
        self.inspecting = Inspecting { uid, names, ids, ..Default::default() };
        if let Some(uid) = uid {
            self.send(ToEngine::Details { uid });
            self.send(ToEngine::Fields { uid });
            self.send(ToEngine::Containers { uid });
        }
    }

    /// Asks again for the data fields of the selected entity, after its data changed.
    pub(super) fn ask_fields(&mut self) {
        if let Some(uid) = self.inspected() {
            self.send(ToEngine::Fields { uid });
        }
    }

    /// The component data of the selected entity, as the engine last sent it.
    fn own_components(&self) -> Option<&Vec<Value>> {
        let uid = self.inspected()?;
        match &self.details {
            Some((of, Value::Array(components))) if *of == uid => Some(components),
            _ => None,
        }
    }

    /// The entity's own data for one component, without its type.
    fn own_data(&self, name: &str) -> Option<Map<String, Value>> {
        let component = self.own_components()?.iter().find(|component| component["type"] == name)?;
        let mut data = component.as_object()?.clone();
        data.remove("type");
        Some(data)
    }

    /// Whether the selected entity's prototype has a component.
    fn prototype_has(&self, name: &str) -> bool {
        let Some(uid) = self.inspected() else { return false };
        let prototype = self.doc.as_ref().and_then(|doc| doc.entities.get(&uid)).and_then(|entity| self.prototype(&entity.prototype));
        prototype.is_some_and(|prototype| prototype.has(name))
    }

    /// The components of the selected entity that have data of its own, in the engine's order,
    /// then those added in the panel. Empty until the engine has sent the data.
    pub fn component_rows(&self) -> Vec<ComponentRow> {
        let Some(components) = self.own_components() else { return Vec::new() };
        let mut rows = Vec::new();
        for component in components {
            let name = component["type"].as_str().unwrap_or("?").to_owned();
            let mut data = component.clone();
            if let Some(fields) = data.as_object_mut() {
                fields.remove("type");
            }
            rows.push(self.row(name, serde_json::to_string_pretty(&data).unwrap_or_default(), false));
        }
        for draft in &self.inspecting.drafts {
            if !rows.iter().any(|row| &row.name == draft) {
                rows.push(self.row(draft.clone(), "{}".to_owned(), true));
            }
        }
        rows
    }

    fn row(&self, name: String, fresh: String, draft: bool) -> ComponentRow {
        let typed = self.inspecting.texts.get(&name);
        ComponentRow {
            edited: typed.is_some_and(|typed| *typed != fresh),
            text: typed.cloned().unwrap_or(fresh),
            draft,
            from_prototype: self.prototype_has(&name),
            reserved: RESERVED_COMPONENTS.contains(&name.as_str()),
            name,
        }
    }

    /// Takes what the user typed into a component's text.
    pub fn set_component_text(&mut self, name: &str, text: String) {
        self.inspecting.texts.insert(name.to_owned(), text);
    }

    /// Puts a component's text back to what the engine sent.
    pub fn revert_component(&mut self, name: &str) {
        self.inspecting.texts.remove(name);
    }

    /// Why the last change of the entity was not made, as the engine or the reading said it.
    pub fn entity_error(&self) -> Option<&str> {
        self.inspecting.error.as_deref()
    }

    /// Whether a change of the entity waits for the engine's answer, so that no other is sent.
    pub fn entity_busy(&self) -> bool {
        self.inspecting.waiting.is_some() || self.inspecting.refreshing
    }

    /// Every component of the selected entity, by name: those the engine told the fields of,
    /// with their forms, and those with data of their own or added in the panel, with their
    /// text. Empty until the engine has sent the data.
    pub fn component_entries(&self) -> Vec<ComponentEntry> {
        let rows = self.component_rows();
        if rows.is_empty() && self.own_components().is_none() {
            return Vec::new();
        }
        let forms: &[ComponentFields] = match &self.inspecting.fields {
            Some(Ok(forms)) => forms,
            _ => &[],
        };
        let names: BTreeSet<&str> = rows.iter().map(|row| row.name.as_str()).chain(forms.iter().map(|form| form.name.as_str())).collect();
        names
            .into_iter()
            .map(|name| {
                let own = self.own_data(name);
                let row = rows.iter().find(|row| row.name == name).cloned().unwrap_or_else(|| self.row(name.to_owned(), "{}".to_owned(), false));
                let fields = forms.iter().find(|form| form.name == name).map(|form| {
                    let own = own.clone().unwrap_or_default();
                    form.fields.iter().map(|field| field_row(field, own.contains_key(&field.tag))).collect()
                });
                ComponentEntry { row, own: own.is_some(), fields }
            })
            .collect()
    }

    /// Why the engine would not tell the fields of the entity's components, if it would not.
    pub fn component_fields_error(&self) -> Option<&str> {
        match &self.inspecting.fields {
            Some(Err(why)) => Some(why),
            _ => None,
        }
    }

    /// A field of a component's form as the engine last told it.
    fn form_field(&self, name: &str, tag: &str) -> Option<DataField> {
        let Some(Ok(forms)) = &self.inspecting.fields else { return None };
        forms.iter().find(|form| form.name == name)?.fields.iter().find(|field| field.tag == tag).cloned()
    }

    /// Whether a prototype id is one the game has, where the session knows the ids of its kind.
    /// One of a kind it does not know is taken.
    fn known_id(&self, kind: Option<&str>, id: &str) -> bool {
        match kind {
            Some("entity") => self.prototype(id).is_some(),
            Some(kind) => self.inspecting.ids.get(kind).is_none_or(|ids| ids.contains(id)),
            None => true,
        }
    }

    /// Sets one field of a component from what was typed into its form. Nothing typed gives it
    /// back to the prototype. The other fields stay.
    pub fn set_component_field(&mut self, name: &str, tag: &str, typed: &str) {
        let Some(field) = self.form_field(name, tag) else { return };
        if typed.trim().is_empty() {
            return self.reset_component_field(name, tag);
        }
        let value = match parse_field(&field, typed) {
            Ok(value) => value,
            Err(why) => return self.entity_refused(format!("{name} {tag} not set: {why}")),
        };
        if let (FieldKind::Prototype, Some(id)) = (field.kind, value.as_str()) {
            if !self.known_id(field.prototype_kind.as_deref(), id) {
                let kind = field.prototype_kind.as_deref().unwrap_or("?");
                return self.entity_refused(format!("{name} {tag} not set: the game has no {kind} prototype {id}"));
            }
        }
        self.set_component_value(name, tag, value);
    }

    /// Sets one field of a component to a value, as a checkbox or a choice gives it. A value
    /// the same as the prototype's is dropped from the entity's own data.
    pub fn set_component_value(&mut self, name: &str, tag: &str, value: Value) {
        let Some(field) = self.form_field(name, tag) else { return };
        if field.read_only {
            return self.entity_refused(format!("{name} {tag} not set: the game never writes it"));
        }
        let before = self.own_data(name);
        if before.as_ref().and_then(|own| own.get(tag)).is_some_and(|own| same_value(own, &value)) {
            return;
        }
        let mut data = before.clone().unwrap_or_default();
        if same_value(&value, &field.prototype) {
            data.remove(tag);
        } else {
            data.insert(tag.to_owned(), value);
        }
        // The same as the prototype where the entity has nothing of its own: nothing changes.
        if Some(&data) == before.as_ref() || (before.is_none() && data.is_empty()) {
            return;
        }
        self.send_components(name, Some(data), "Edit component");
    }

    /// Sets or clears one flag of a set of flags.
    pub fn set_component_flag(&mut self, name: &str, tag: &str, flag: &str, on: bool) {
        let Some(field) = self.form_field(name, tag) else { return };
        self.set_component_value(name, tag, with_flag(&field, flag, on));
    }

    /// Gives one field of a component back to the prototype: the entity's own data without it.
    pub fn reset_component_field(&mut self, name: &str, tag: &str) {
        let Some(mut data) = self.own_data(name) else { return };
        if data.remove(tag).is_none() {
            return;
        }
        self.send_components(name, Some(data), "Reset field");
    }

    /// Notes why a change was not made, and says so on the status line.
    fn entity_refused(&mut self, why: String) {
        self.set_status(why.clone());
        self.inspecting.error = Some(why);
    }

    /// The entity a change goes to and its component data, if a change can be sent now. Else
    /// the status line says why not.
    fn editable(&mut self, needs_data: bool) -> Option<(i32, Vec<Value>)> {
        let uid = self.inspected()?;
        if self.entity_busy() {
            self.set_status("Waiting for the engine to finish this entity's last change");
            return None;
        }
        match self.own_components() {
            Some(components) => Some((uid, components.clone())),
            None if needs_data => {
                self.set_status("The engine hasn't sent this entity's components yet");
                None
            }
            None => Some((uid, Vec::new())),
        }
    }

    /// Sends an edit of the selected entity, as one undo step.
    fn send_entity_edit(&mut self, op: Value, label: &'static str, component: Option<String>) {
        self.inspecting.error = None;
        let seq = self.edit(vec![op], label, None, false);
        self.inspecting.waiting = Some(Waiting { seq, component });
    }

    /// Sends the entity's list of components with one of them set to this data, or with it
    /// taken out. The others go as the engine sent them, so they stay as they are.
    fn send_components(&mut self, name: &str, data: Option<Map<String, Value>>, label: &'static str) {
        let Some((uid, mut components)) = self.editable(true) else { return };
        let at = components.iter().position(|component| component["type"] == name);
        let replaced = data.map(|data| {
            let mut component = Map::from_iter([("type".to_owned(), json!(name))]);
            component.extend(data);
            Value::Object(component)
        });
        match (at, replaced) {
            (Some(at), Some(component)) => components[at] = component,
            (Some(at), None) => {
                components.remove(at);
            }
            (None, Some(component)) => components.push(component),
            (None, None) => {}
        }
        let op = json!({ "kind": "components", "uid": uid, "components": Value::Array(components).to_string() });
        self.send_entity_edit(op, label, Some(name.to_owned()));
    }

    /// Applies a component's text. Text that is not one JSON object is refused before anything
    /// is sent, and the text stays for the user to mend.
    pub fn apply_component(&mut self, name: &str) {
        let draft = self.inspecting.drafts.iter().any(|draft| draft == name);
        let text = match self.inspecting.texts.get(name) {
            Some(text) => text.clone(),
            None if draft => "{}".to_owned(),
            None => return,
        };
        match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(data)) => self.send_components(name, Some(data), if draft { "Add component" } else { "Edit component" }),
            Ok(_) => self.entity_refused(format!("{name} not applied: the text must be a single JSON object in braces")),
            Err(error) => self.entity_refused(format!("{name} not applied: the text isn't valid JSON: {error}")),
        }
    }

    /// Adds a component by name. One the prototype has already becomes a draft, since adding
    /// it with no fields would change nothing: it is sent when it has fields and is applied.
    pub fn add_component(&mut self, typed: &str) {
        let name = typed.trim();
        if name.is_empty() {
            return self.entity_refused("No component added: type its name".to_owned());
        }
        if RESERVED_COMPONENTS.contains(&name) {
            return self.entity_refused(format!("{name} not added: moving and renaming change it"));
        }
        if self.own_data(name).is_some() || self.inspecting.drafts.iter().any(|draft| draft == name) {
            return self.entity_refused(format!("{name} not added: the entity already has it"));
        }
        if self.prototype_has(name) {
            self.inspecting.error = None;
            self.inspecting.drafts.push(name.to_owned());
            return self.set_status(format!("{name} comes from the prototype. Add the fields you want to change, then Apply"));
        }
        self.send_components(name, Some(Map::new()), "Add component");
    }

    /// Takes a component's own data away. One the prototype has goes back to the prototype's.
    /// A draft is only dropped.
    pub fn remove_component(&mut self, name: &str) {
        if let Some(at) = self.inspecting.drafts.iter().position(|draft| draft == name) {
            self.inspecting.drafts.remove(at);
            self.inspecting.texts.remove(name);
            return;
        }
        if self.own_data(name).is_none() {
            return;
        }
        let label = if self.prototype_has(name) { "Reset component" } else { "Remove component" };
        self.send_components(name, None, label);
    }

    /// Renames the selected entity. Nothing typed takes its own name away.
    pub fn rename_entity(&mut self, typed: &str) {
        let Some((uid, _)) = self.editable(false) else { return };
        let op = json!({ "kind": "rename", "uid": uid, "name": typed.trim() });
        self.send_entity_edit(op, "Rename", None);
    }

    /// The component names of every prototype that have every word of a search in them, in
    /// either case, sorted, at most `limit` of them.
    pub fn component_names_matching(&mut self, search: &str, limit: usize) -> Vec<Arc<str>> {
        if self.inspecting.names.is_empty() {
            let names: HashSet<Arc<str>> = self.prototypes.iter().flat_map(|prototype| prototype.components.iter().cloned()).collect();
            let mut names: Vec<Arc<str>> = names.into_iter().collect();
            names.sort_unstable();
            self.inspecting.names = names;
        }
        let words: Vec<String> = search.split_whitespace().map(str::to_lowercase).collect();
        let found = self.inspecting.names.iter().filter(|name| {
            let lower = name.to_lowercase();
            words.iter().all(|word| lower.contains(word.as_str()))
        });
        found.take(limit).cloned().collect()
    }

    /// The selected entity's containers as the engine last told them, or why it would not.
    /// None until it has answered.
    pub fn containers(&self) -> Option<&Result<Vec<Container>, String>> {
        self.inspecting.containers.as_ref()
    }

    /// Makes a new entity of a prototype inside one of the selected entity's containers.
    pub fn contain(&mut self, container: &str, prototype: &str) {
        let Some((uid, _)) = self.editable(false) else { return };
        let known = self.containers().and_then(|containers| containers.as_ref().ok()).and_then(|containers| containers.iter().find(|known| known.id == container));
        match known {
            None => return self.entity_refused(format!("Not added: the entity has no container {container}")),
            Some(known) if !known.can_insert => return self.entity_refused(format!("Not added: {container} is full")),
            Some(_) => {}
        }
        let op = json!({ "kind": "contain", "parent": uid, "container": container, "prototype": prototype });
        self.send_entity_edit(op, "Add to container", None);
    }

    /// Deletes something the selected entity holds, with whatever it holds itself.
    pub fn uncontain(&mut self, held: i32) {
        if self.editable(false).is_none() {
            return;
        }
        self.send_entity_edit(json!({ "kind": "uncontain", "uid": held }), "Remove from container", None);
    }

    /// The engine has answered an edit or an undo. The answer to the last change of the
    /// entity ends the wait: a refusal is kept whole for the panel, and a change that was made
    /// drops the text it applied.
    pub(super) fn entity_answered(&mut self, seq: u64, refused: Option<&str>) {
        let Some(waiting) = self.inspecting.waiting.take_if(|waiting| waiting.seq <= seq) else { return };
        if waiting.seq != seq {
            return;
        }
        match refused {
            Some(error) => self.inspecting.error = Some(error.to_owned()),
            None => {
                if let Some(name) = waiting.component {
                    self.inspecting.texts.remove(&name);
                    self.inspecting.drafts.retain(|draft| *draft != name);
                }
            }
        }
    }

    /// Before a change is taken into the mirror: whether it touches the selected entity, so
    /// that its data or its containers are to be asked for again. An undo or a redo may change
    /// anything, so both are asked again after one.
    pub(super) fn entity_changes(&self, delta: &Delta, what: &str) -> (bool, bool) {
        let Some(uid) = self.inspected() else { return (false, false) };
        if what == "Undo" || what == "Redo" {
            return (true, true);
        }
        let held = |gone: &i32| {
            let known = self.inspecting.containers.as_ref().and_then(|containers| containers.as_ref().ok());
            known.is_some_and(|containers| containers.iter().any(|container| container.entities.contains(gone)))
        };
        // Not on a move of the entity itself, which a drag sends many of.
        let containers = delta.details.iter().any(|details| details.uid == uid)
            || delta.entities.iter().any(|entity| entity.contained && entity.parent == uid)
            || delta.removed.iter().any(held);
        // A rename changes the name in the data, and the engine sends no data for it.
        let details = delta.entities.iter().any(|entity| entity.uid == uid) && !delta.details.iter().any(|details| details.uid == uid);
        (details && what == "Rename", containers)
    }

    /// Asks again for the selected entity's data, its containers, or both.
    pub(super) fn ask_again(&mut self, details: bool, containers: bool) {
        let Some(uid) = self.inspected() else { return };
        if details {
            self.inspecting.refreshing = true;
            self.send(ToEngine::Details { uid });
            self.send(ToEngine::Fields { uid });
        }
        if containers {
            self.send(ToEngine::Containers { uid });
        }
    }

    /// The engine sent an entity's component data.
    pub(super) fn details_arrived(&mut self, uid: i32, components: Value) {
        if self.inspected() == Some(uid) {
            self.details = Some((uid, components));
            self.inspecting.refreshing = false;
        }
    }

    /// The engine told the data fields of an entity's components, with the ids of the
    /// prototype kinds they name that it had not told before.
    pub(super) fn fields_arrived(&mut self, uid: i32, answer: Result<FieldsAnswer, String>) {
        if self.inspected() != Some(uid) {
            return;
        }
        self.inspecting.fields = Some(answer.map(|answer| {
            for (kind, ids) in answer.ids {
                self.inspecting.ids.insert(kind, ids.into_iter().collect());
            }
            answer.components
        }));
    }

    /// The engine told an entity's containers.
    pub(super) fn containers_arrived(&mut self, uid: i32, containers: Result<Vec<Container>, String>) {
        if self.inspected() == Some(uid) {
            self.inspecting.containers = Some(containers);
        }
    }

    /// The entity the session keeps data for.
    pub(super) fn inspecting_uid(&self) -> Option<i32> {
        self.inspecting.uid
    }
}

/// A field as the panel shows it.
fn field_row(field: &DataField, own: bool) -> FieldRow {
    FieldRow {
        tag: field.tag.clone(),
        kind: field.kind,
        type_name: field.type_name.clone(),
        text: field_text(field.kind, &field.value),
        value: field.value.clone(),
        prototype: field.prototype.clone(),
        own,
        read_only: field.read_only,
        names: field.names().to_vec(),
        prototype_kind: field.prototype_kind.clone(),
    }
}
