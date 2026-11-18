//! The device link tool: linking a source's ports to a sink's, and putting devices on a
//! device list, as the web editor's Link tool does, with the engine making each change.
//!
//! A press on an entity that has links or keeps a device list takes it as the source, selects
//! it and asks the engine its ports. One that has both is taken for its list, as the web
//! editor does, and with Shift for its links. A press on another entity then works on the
//! pair: for a list it adds the device (`list.add`), or offers to take it off if it is on the
//! list already, and for links it opens the chooser of ports for the pair, which asks the
//! engine the ports both have now (`ports.of`). The chooser offers the source's ports and the
//! sink's ports with the game's defaults first (`crate::links`), and the links the pair has
//! already, each with Unlink. Each change is one edit and one step to undo. The engine's
//! refusal is kept whole for the chooser and the mirror stays as it was. A secondary press on
//! a listed device takes it off the list. A press on nothing, Escape and a change of tool end
//! linking. Nothing is dragged.

use super::*;
use crate::links::{alarm_of, room_devices, sink_ports_ordered, source_ports_ordered};
use crate::model::Link;

/// The links and list entries of one entity, as the selection panel lists them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityLinkRows {
    /// The links it makes as a source, and those that end at it.
    pub outgoing: Vec<Link>,
    pub incoming: Vec<Link>,
    /// The devices on its own list, and the entities whose lists have it.
    pub devices: Vec<i32>,
    pub lists: Vec<i32>,
    /// Whether it is an alarm with a device list, which Auto-link Room works on.
    pub alarm: bool,
}

/// What a source is linked by: its ports, or its device list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Links,
    List,
}

/// What the link tool keeps between presses.
#[derive(Default)]
pub(in crate::session) struct Linking {
    /// The entity links are made from, and what it is linked by.
    source: Option<(i32, LinkKind)>,
    /// The other end of a pair being worked on, while the chooser is open.
    chosen: Option<Chosen>,
    /// The ports of each entity as the engine last told them.
    ports: HashMap<i32, EntityPorts>,
    /// The last change the tool sent, until the engine answers it.
    waiting: Option<u64>,
    /// Why the engine would not make the last change or tell the ports, whole.
    error: Option<String>,
}

struct Chosen {
    sink: i32,
    /// The ports chosen in the chooser. None takes the first offered.
    source_port: Option<String>,
    sink_port: Option<String>,
}

impl Linking {
    /// Ends linking: no source and no chooser.
    pub(super) fn end(&mut self) {
        self.source = None;
        self.chosen = None;
        self.error = None;
    }
}

/// The chooser of a pair, for the interface to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkChooser {
    pub source: i32,
    pub sink: i32,
    pub kind: LinkKind,
    /// The source's ports and the sink's, those the game links by default first.
    pub source_ports: Vec<String>,
    pub sink_ports: Vec<String>,
    /// The ports a link would be made with.
    pub source_port: Option<String>,
    pub sink_port: Option<String>,
    /// The links the pair has already.
    pub existing: Vec<Link>,
    /// Whether the sink is on the source's device list.
    pub listed: bool,
    /// The engine has not told the ports of both yet.
    pub asking: bool,
    /// A change waits for the engine's answer.
    pub waiting: bool,
    /// Why the engine would not make the last change or tell the ports, as it said it.
    pub error: Option<String>,
}

/// The dashed line the interface draws from the source to the cursor, or to the other end of
/// the pair in the chooser.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkLine {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub kind: LinkKind,
}

impl Session {
    /// Whether an entity has links and whether it keeps a device list, as far as the
    /// prototypes, the engine's ports and the mirror tell.
    fn link_facts(&self, uid: i32) -> (bool, bool) {
        let Some(entity) = self.doc.as_ref().and_then(|doc| doc.entities.get(&uid)) else { return (false, false) };
        let prototype = self.prototype(&entity.prototype);
        let told = self.linking.ports.get(&uid);
        let mirror = self.doc.as_ref().and_then(|doc| doc.links.as_ref());
        let links = self.ports.is_source(&entity.prototype)
            || prototype.is_some_and(|prototype| prototype.has("DeviceLinkSource"))
            || told.is_some_and(|told| !told.sources.is_empty())
            || mirror.is_some_and(|mirror| !mirror.links_from(uid).is_empty());
        let list = prototype.is_some_and(|prototype| prototype.has("DeviceList"))
            || told.is_some_and(|told| told.has_device_list)
            || mirror.is_some_and(|mirror| !mirror.devices(uid).is_empty());
        (links, list)
    }

    /// What an entity would be linked by as a source, if it can be one: its list where it
    /// keeps one, unless `links_first` and it has links too.
    fn link_kind_of(&self, uid: i32, links_first: bool) -> Option<LinkKind> {
        match self.link_facts(uid) {
            (true, true) if links_first => Some(LinkKind::Links),
            (_, true) => Some(LinkKind::List),
            (true, false) => Some(LinkKind::Links),
            (false, false) => None,
        }
    }

    /// Whether a device is on a list, as the mirror has it.
    fn listed(&self, list: i32, device: i32) -> bool {
        self.doc.as_ref().and_then(|doc| doc.links.as_ref()).is_some_and(|mirror| mirror.devices(list).contains(&device))
    }

    /// The entity a link is made from, and what it is linked by, while linking.
    pub fn link_source(&self) -> Option<(i32, LinkKind)> {
        self.linking.source
    }

    /// A press of the link tool.
    pub(super) fn link_down(&mut self, world: (f32, f32)) {
        let found = self.entities_at(world);
        let Some((source, kind)) = self.linking.source else {
            let shift = self.held.shift;
            if let Some((uid, kind)) = found.iter().find_map(|&uid| self.link_kind_of(uid, shift).map(|kind| (uid, kind))) {
                self.begin_linking(uid, kind);
            }
            return;
        };
        let Some(&target) = found.iter().find(|&&uid| uid != source) else {
            // A press on nothing ends linking, and one on the source alone does nothing.
            if found.is_empty() {
                self.cancel_linking();
                self.deselect();
            }
            return;
        };
        match kind {
            LinkKind::List if self.listed(source, target) => self.choose_pair(target),
            LinkKind::List => self.list_add(source, target),
            LinkKind::Links => self.choose_pair(target),
        }
    }

    /// A secondary press of the link tool: takes a listed device off the list, or opens the
    /// chooser of a pair that has links. Nothing without a source.
    pub(super) fn link_secondary_down(&mut self, world: (f32, f32)) {
        let Some((source, kind)) = self.linking.source else { return };
        let found = self.entities_at(world);
        let Some(&target) = found.iter().find(|&&uid| uid != source) else {
            if found.is_empty() {
                self.cancel_linking();
            }
            return;
        };
        let linked = self.doc.as_ref().and_then(|doc| doc.links.as_ref()).is_some_and(|mirror| mirror.links_from(source).iter().any(|link| link.sink == target));
        match kind {
            LinkKind::List if self.listed(source, target) => self.list_remove(source, target),
            LinkKind::Links if linked => self.choose_pair(target),
            LinkKind::List | LinkKind::Links => {}
        }
    }

    fn begin_linking(&mut self, uid: i32, kind: LinkKind) {
        self.linking.end();
        self.linking.source = Some((uid, kind));
        self.selection = vec![uid];
        self.selected_decals.clear();
        self.send(ToEngine::PortsOf { uids: vec![uid] });
        let what = match kind {
            LinkKind::Links => "click a device to link it",
            LinkKind::List => "click a device to put it on the list",
        };
        self.set_status(format!("Linking from {}: {what}. Press Escape to stop", self.entity_label(uid)));
    }

    /// Opens the chooser for the source and another entity, and asks the ports both have now.
    fn choose_pair(&mut self, sink: i32) {
        let Some((source, kind)) = self.linking.source else { return };
        self.linking.chosen = Some(Chosen { sink, source_port: None, sink_port: None });
        self.linking.error = None;
        if kind == LinkKind::Links {
            self.send(ToEngine::PortsOf { uids: vec![source, sink] });
        }
    }

    /// The engine told ports, or would not.
    pub(in crate::session) fn ports_arrived(&mut self, uids: &[i32], ports: Result<Vec<EntityPorts>, String>) {
        match ports {
            Ok(ports) => {
                for told in ports {
                    self.linking.ports.insert(told.uid, told);
                }
            }
            Err(error) => {
                if self.linking.chosen.as_ref().is_some_and(|chosen| uids.contains(&chosen.sink)) {
                    self.linking.error = Some(format!("The engine couldn't list the ports: {error}"));
                }
            }
        }
    }

    /// Ends linking. False if it was not on.
    pub fn cancel_linking(&mut self) -> bool {
        let on = self.linking.source.is_some();
        self.linking.end();
        on
    }

    /// Closes the chooser and keeps the source.
    pub fn close_link_chooser(&mut self) {
        self.linking.chosen = None;
        self.linking.error = None;
    }

    /// The chooser of the pair being worked on, while it is open.
    pub fn link_chooser(&self) -> Option<LinkChooser> {
        let (source, kind) = self.linking.source?;
        let chosen = self.linking.chosen.as_ref()?;
        let mirror = self.doc.as_ref()?.links.as_ref();
        let (source_told, sink_told) = (self.linking.ports.get(&source), self.linking.ports.get(&chosen.sink));
        let sinks = sink_told.map_or_else(Vec::new, |told| told.sinks.clone());
        let source_ports = source_told.map_or_else(Vec::new, |told| source_ports_ordered(&self.ports, &told.sources, &sinks));
        let source_port = chosen.source_port.clone().filter(|port| source_ports.contains(port)).or_else(|| source_ports.first().cloned());
        let sink_ports = sink_ports_ordered(&self.ports, source_port.as_deref(), &sinks);
        let sink_port = chosen.sink_port.clone().filter(|port| sink_ports.contains(port)).or_else(|| sink_ports.first().cloned());
        Some(LinkChooser {
            source,
            sink: chosen.sink,
            kind,
            existing: mirror.map_or_else(Vec::new, |mirror| mirror.links_from(source).iter().filter(|link| link.sink == chosen.sink).cloned().collect()),
            listed: self.listed(source, chosen.sink),
            asking: kind == LinkKind::Links && (source_told.is_none() || sink_told.is_none()),
            waiting: self.linking.waiting.is_some(),
            error: self.linking.error.clone(),
            source_ports,
            sink_ports,
            source_port,
            sink_port,
        })
    }

    /// Chooses the source's port in the chooser. The sink's goes back to the first offered for it.
    pub fn choose_source_port(&mut self, port: &str) {
        if let Some(chosen) = self.linking.chosen.as_mut() {
            chosen.source_port = Some(port.to_owned());
            chosen.sink_port = None;
        }
    }

    /// Chooses the sink's port in the chooser.
    pub fn choose_sink_port(&mut self, port: &str) {
        if let Some(chosen) = self.linking.chosen.as_mut() {
            chosen.sink_port = Some(port.to_owned());
        }
    }

    /// Links the pair in the chooser with the ports chosen.
    pub fn make_link(&mut self) {
        let Some(chooser) = self.link_chooser() else { return };
        let (Some(port), Some(target_port)) = (chooser.source_port, chooser.sink_port) else {
            return self.set_status("Not linked: both need a port");
        };
        let op = json!({ "kind": "link", "uid": chooser.source, "port": port, "target": chooser.sink, "targetPort": target_port });
        self.link_edit(op, "Link");
    }

    /// Takes a link away.
    pub fn unlink(&mut self, link: &Link) {
        let op = json!({ "kind": "unlink", "uid": link.source, "port": link.source_port, "target": link.sink, "targetPort": link.sink_port });
        self.link_edit(op, "Unlink");
    }

    /// Puts a device on an entity's device list.
    pub fn list_add(&mut self, list: i32, device: i32) {
        self.link_edit(json!({ "kind": "list.add", "uid": list, "target": device }), "Add to device list");
    }

    /// Takes a device off an entity's device list.
    pub fn list_remove(&mut self, list: i32, device: i32) {
        self.link_edit(json!({ "kind": "list.remove", "uid": list, "target": device }), "Remove from device list");
    }

    /// Sends a change of links as one edit, which is one step to undo.
    fn link_edit(&mut self, op: Value, label: &'static str) {
        self.linking.error = None;
        let seq = self.edit(vec![op], label, None, false);
        self.linking.waiting = Some(seq);
    }

    /// The engine answered an edit or an undo. The answer to the tool's last change ends the
    /// wait, and a refusal is kept whole.
    pub(in crate::session) fn link_answered(&mut self, seq: u64, refused: Option<&str>) {
        let Some(waiting) = self.linking.waiting.take_if(|waiting| *waiting <= seq) else { return };
        if waiting == seq {
            self.linking.error = refused.map(str::to_owned);
        }
    }

    /// After a change was taken into the mirror: a source that is gone ends linking, and an
    /// other end that is gone closes the chooser.
    pub(in crate::session) fn linking_checked(&mut self) {
        let Some(doc) = self.doc.as_ref() else { return };
        if self.linking.source.is_some_and(|(source, _)| !doc.entities.contains_key(&source)) {
            self.linking.end();
        }
        if self.linking.chosen.as_ref().is_some_and(|chosen| !doc.entities.contains_key(&chosen.sink)) {
            self.linking.chosen = None;
        }
    }

    /// The links and list entries of one entity, from either end, for the selection panel.
    /// None while the mirror holds no links.
    pub fn entity_link_rows(&self, uid: i32) -> Option<EntityLinkRows> {
        let doc = self.doc.as_ref()?;
        let mirror = doc.links.as_ref()?;
        let alarm = doc.entities.get(&uid).is_some_and(|entity| alarm_of(&entity.prototype).is_some()) && self.link_facts(uid).1;
        Some(EntityLinkRows {
            outgoing: mirror.links_from(uid).to_vec(),
            incoming: mirror.links_to(uid).into_iter().cloned().collect(),
            devices: mirror.devices(uid).to_vec(),
            lists: mirror.lists_with(uid),
            alarm,
        })
    }

    /// The web editor's Auto-link Room for the one selected entity, an air alarm or a fire
    /// alarm: the devices of its room go on its list in one edit (`crate::links::room_devices`).
    pub fn auto_link_room(&mut self) {
        let [alarm] = self.selection[..] else { return };
        let Some(doc) = self.doc.as_ref() else { return };
        let Some(mirror) = doc.links.as_ref() else { return self.set_status("Not linked: the engine hasn't listed the links yet") };
        match room_devices(doc, mirror, alarm, |id| self.prototype(id)) {
            Err(why) => self.set_status(format!("Not linked: {why}")),
            Ok(devices) if devices.is_empty() => self.set_status("No new devices found"),
            Ok(devices) => {
                let count = devices.len();
                let ops = devices.into_iter().map(|device| json!({ "kind": "list.add", "uid": alarm, "target": device })).collect();
                let seq = self.edit(ops, "Auto-link room", None, false);
                let devices = if count == 1 { "device" } else { "devices" };
                self.set_status(format!("Linking {count} {devices}"));
                // The web editor's message once it is done.
                self.edit_says = Some((seq, format!("Linked {count} {devices}")));
            }
        }
    }

    /// The name of an entity for the interface: its own, or its prototype's, or else the
    /// prototype's id.
    pub fn entity_label(&self, uid: i32) -> String {
        let Some(entity) = self.doc.as_ref().and_then(|doc| doc.entities.get(&uid)) else { return format!("#{uid}") };
        let prototype = self.prototype(&entity.prototype).map(|prototype| prototype.name.as_str()).filter(|name| !name.is_empty());
        entity.name.clone().filter(|name| !name.is_empty()).unwrap_or_else(|| prototype.unwrap_or(&entity.prototype).to_owned())
    }

    /// The English name of a port, a sink port's or a source port's, from the fork's `.ftl`
    /// files by the key the engine told. None till they are read, and for a port whose key has
    /// no plain text: the interface then shows its id.
    pub fn port_name(&self, port: &str, sink: bool) -> Option<&str> {
        self.locale.get(self.ports.name_key(port, sink)?)
    }

    /// A port as the interface shows it: by its name, and by its id when that differs from the
    /// name, for a muted note. The id alone where no name is known.
    pub fn port_shown(&self, port: &str, sink: bool) -> (String, Option<String>) {
        match self.port_name(port, sink) {
            Some(name) if name != port => (name.to_owned(), Some(port.to_owned())),
            _ => (port.to_owned(), None),
        }
    }

    /// A pair of ports as the interface writes a link: by their names, "Pressed → Toggle", and
    /// by their ids when those differ from the names, for a muted note. The ids alone where no
    /// names are known.
    pub fn link_ports_text(&self, source_port: &str, sink_port: &str) -> (String, Option<String>) {
        let ids = format!("{source_port} \u{2192} {sink_port}");
        let names = format!("{} \u{2192} {}", self.port_name(source_port, false).unwrap_or(source_port), self.port_name(sink_port, true).unwrap_or(sink_port));
        if names == ids {
            (ids, None)
        } else {
            (names, Some(ids))
        }
    }

    /// The outline of the cell an entity is on, through whatever holds it.
    fn entity_cell(&self, uid: i32) -> Option<[(f32, f32); 4]> {
        let doc = self.doc.as_ref()?;
        let (grid, local) = doc.place_of(uid)?;
        let cell = (local.0.floor() as i32, local.1.floor() as i32);
        Some(match doc.grid(grid) {
            Some(grid) => grid.cells_outline(cell, cell),
            None => boxed((cell.0 as f32, cell.1 as f32, cell.0 as f32 + 1.0, cell.1 as f32 + 1.0)),
        })
    }

    /// The link tool's outlines, as the web editor's: with no source every entity that could
    /// be one, faintly, and while linking the source brightly and what it is linked to.
    pub(super) fn link_outlines(&self, outlines: &mut Vec<Outline>) {
        let Some(doc) = self.doc.as_ref() else { return };
        let mut push = |uid: i32, style: OutlineStyle| {
            if let Some(corners) = self.entity_cell(uid) {
                outlines.push(Outline { corners, style });
            }
        };
        let Some((source, kind)) = self.linking.source else {
            let mut sources: Vec<i32> = doc.entities.values().filter(|entity| !entity.contained).map(|entity| entity.uid).filter(|&uid| self.link_kind_of(uid, false).is_some()).collect();
            sources.sort_unstable();
            for uid in sources {
                push(uid, OutlineStyle::Linkable);
            }
            return;
        };
        push(source, OutlineStyle::LinkSource);
        let mut linked: Vec<i32> = match (kind, doc.links.as_ref()) {
            (LinkKind::Links, Some(mirror)) => mirror.links_from(source).iter().map(|link| link.sink).collect(),
            (LinkKind::List, Some(mirror)) => mirror.devices(source).to_vec(),
            (_, None) => Vec::new(),
        };
        linked.sort_unstable();
        linked.dedup();
        for uid in linked {
            push(uid, OutlineStyle::Linked);
        }
        if let Some(chosen) = self.linking.chosen.as_ref() {
            push(chosen.sink, OutlineStyle::LinkSource);
        }
    }

    /// The dashed line from the source, while linking.
    pub fn link_line(&self) -> Option<LinkLine> {
        let (source, kind) = self.linking.source?;
        if self.tool != Tool::Link {
            return None;
        }
        let doc = self.doc.as_ref()?;
        let from = doc.world_of(source)?;
        let to = match self.linking.chosen.as_ref() {
            Some(chosen) => doc.world_of(chosen.sink)?,
            None => self.hover?,
        };
        Some(LinkLine { from, to, kind })
    }
}
