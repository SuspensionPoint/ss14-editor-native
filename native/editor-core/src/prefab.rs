//! Prefabs: a clipboard region saved to a file, and the folder they live in.
//!
//! A prefab file starts with one line of JSON, the header. After it comes the engine's own text
//! of the entities, to the end of the file, byte for byte as the engine wrote it. The header holds
//! the format's name and version, the origin cell the text is measured from, the counts, and the
//! region's cells, tiles and decals measured from the origin cell, just as the clipboard holds
//! them. The editor never reads the entity text itself. A file from a newer version, or a broken
//! one, is refused with a message and never partly read.
//!
//! The folder sits beside the editor's other user data and can have subfolders. A prefab is named
//! by its path inside the folder, with `/` between the parts and no extension.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::Decal;
use crate::session::CopiedTile;

/// The extension of a prefab file.
pub const EXTENSION: &str = "ss14prefab";
/// The version of the format this editor writes, and the newest it reads.
pub const VERSION: u64 = 1;
/// The format's name, the first thing in the header.
const FORMAT: &str = "ss14-editor-prefab";
/// How many squares a side of a prefab's preview has at most.
const PREVIEW_SIDE: usize = 24;

/// A region as a prefab file holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefab {
    /// The cell of the grid the region was copied from that the entity text is measured from.
    pub from: (i32, i32),
    pub roots: usize,
    pub entities: usize,
    /// The cells, tiles and decals measured from the origin cell, as the clipboard has them.
    pub cells: Vec<(i32, i32)>,
    pub tiles: Vec<CopiedTile>,
    pub decals: Vec<Decal>,
    /// The engine's own text of the entities. Empty with none.
    pub text: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    format: String,
    version: u64,
    from: (i32, i32),
    roots: usize,
    entities: usize,
    cells: Vec<(i32, i32)>,
    tiles: Vec<TileLine>,
    decals: Vec<DecalLine>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TileLine {
    cell: (i32, i32),
    tile: String,
    variant: u8,
    rotation_mirroring: u8,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DecalLine {
    id: i32,
    prototype: String,
    x: f32,
    y: f32,
    color: Option<String>,
    angle: f64,
    z_index: i32,
    cleanable: bool,
}

impl Prefab {
    /// The file's text: the header on one line, then the entity text as it is.
    pub fn write(&self) -> String {
        let header = Header {
            format: FORMAT.to_owned(),
            version: VERSION,
            from: self.from,
            roots: self.roots,
            entities: self.entities,
            cells: self.cells.clone(),
            tiles: self.tiles.iter().map(|tile| TileLine { cell: tile.cell, tile: tile.tile.clone(), variant: tile.variant, rotation_mirroring: tile.rotation_mirroring }).collect(),
            decals: self
                .decals
                .iter()
                .map(|decal| DecalLine {
                    id: decal.id,
                    prototype: decal.prototype.clone(),
                    x: decal.x,
                    y: decal.y,
                    color: decal.color.clone(),
                    angle: decal.angle,
                    z_index: decal.z_index,
                    cleanable: decal.cleanable,
                })
                .collect(),
        };
        // The header has no line break in it: JSON writes one in a string as \n.
        let header = serde_json::to_string(&header).expect("a header is always written");
        format!("{header}\n{}", self.text)
    }

    /// Reads a prefab file. The message of a refusal says what is wrong, for the user.
    pub fn read(file: &[u8]) -> Result<Prefab, String> {
        let (line, text) = match file.iter().position(|&byte| byte == b'\n') {
            Some(end) => (&file[..end], &file[end + 1..]),
            None => (file, &[][..]),
        };
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let header: Value = serde_json::from_slice(line).map_err(|_| "it isn't a prefab file, or it's damaged".to_owned())?;
        if header.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Err("it isn't a prefab file from this editor".to_owned());
        }
        match header.get("version").and_then(Value::as_u64) {
            Some(version) if version > VERSION => return Err(format!("it was made by a newer editor (format {version}, this editor reads up to {VERSION})")),
            Some(1..) => {}
            _ => return Err("its header has no version".to_owned()),
        }
        let header: Header = serde_json::from_value(header).map_err(|error| format!("its header is broken ({error})"))?;
        let text = String::from_utf8(text.to_vec()).map_err(|_| "its entity text isn't UTF-8".to_owned())?;
        if header.cells.is_empty() {
            return Err("it has no cells".to_owned());
        }
        if header.tiles.iter().any(|tile| !header.cells.contains(&tile.cell)) {
            return Err("it has a tile outside its cells".to_owned());
        }
        Ok(Prefab {
            from: header.from,
            roots: header.roots,
            entities: header.entities,
            cells: header.cells,
            tiles: header.tiles.into_iter().map(|tile| CopiedTile { cell: tile.cell, tile: tile.tile, variant: tile.variant, rotation_mirroring: tile.rotation_mirroring }).collect(),
            decals: header
                .decals
                .into_iter()
                .map(|decal| Decal { id: decal.id, prototype: decal.prototype, x: decal.x, y: decal.y, color: decal.color, angle: decal.angle, z_index: decal.z_index, cleanable: decal.cleanable })
                .collect(),
            text,
        })
    }

    /// How many cells wide and high the box around its cells is.
    pub fn size(&self) -> (i32, i32) {
        size_of(&self.cells)
    }
}

/// How many cells wide and high the box around some cells is. (0, 0) with none.
fn size_of(cells: &[(i32, i32)]) -> (i32, i32) {
    if cells.is_empty() {
        return (0, 0);
    }
    let (left, bottom, right, top) = cells.iter().fold((i32::MAX, i32::MAX, i32::MIN, i32::MIN), |(l, b, r, t), &(x, y)| (l.min(x), b.min(y), r.max(x), t.max(y)));
    (right - left + 1, top - bottom + 1)
}

/// Where prefabs are kept: beside the backdrop cache, in the user's own data.
pub fn default_folder() -> PathBuf {
    crate::files::user_data().join("prefabs")
}

/// A small picture of a prefab's cells: a box of squares at most `PREVIEW_SIDE` a side, the
/// region's shape in it, row by row from the top. A square is filled when a cell of the
/// region falls in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    pub width: usize,
    pub height: usize,
    pub filled: Vec<bool>,
}

impl Preview {
    fn of(cells: &[(i32, i32)]) -> Preview {
        let (width, height) = size_of(cells);
        let (width, height) = (width.max(1) as usize, height.max(1) as usize);
        // One square stands for this many cells each way, the same both ways.
        let step = width.max(height).div_ceil(PREVIEW_SIDE);
        let (side_x, side_y) = (width.div_ceil(step), height.div_ceil(step));
        let mut filled = vec![false; side_x * side_y];
        let (left, bottom) = cells.iter().fold((i32::MAX, i32::MAX), |(l, b), &(x, y)| (l.min(x), b.min(y)));
        for &(x, y) in cells {
            let (col, row) = ((x - left) as usize / step, (y - bottom) as usize / step);
            filled[(side_y - 1 - row) * side_x + col] = true;
        }
        Preview { width: side_x, height: side_y, filled }
    }
}

/// What the Prefabs tab shows of a prefab, read from its file.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub size: (i32, i32),
    pub tiles: usize,
    pub entities: usize,
    pub preview: Preview,
}

/// A prefab in the folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    /// Its path from the folder with `/` between the parts and no extension, which names it.
    pub name: String,
    /// The subfolder it is in, as its name has it. Empty at the top of the folder.
    pub group: String,
    /// The last part of its name.
    pub title: String,
    /// What its file holds, or why it cannot be read.
    pub shape: Result<Shape, String>,
}

/// Every prefab file in a folder and its subfolders, those at the top first and then each
/// subfolder's in the order of their names, each by name. A folder that is not there has none.
pub fn list(folder: &Path) -> Vec<Listed> {
    let mut listed = Vec::new();
    walk(folder, "", &mut listed);
    listed.sort_by_cached_key(|prefab| (!prefab.group.is_empty(), prefab.group.to_lowercase(), prefab.title.to_lowercase()));
    listed
}

fn walk(folder: &Path, group: &str, listed: &mut Vec<Listed>) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else { continue };
        let joined = |part: &str| if group.is_empty() { part.to_owned() } else { format!("{group}/{part}") };
        if path.is_dir() {
            walk(&path, &joined(file_name), listed);
            continue;
        }
        let Some(title) = strip_extension(file_name) else { continue };
        let shape = fs::read(&path).map_err(|error| error.to_string()).and_then(|bytes| Prefab::read(&bytes)).map(|prefab| Shape {
            size: prefab.size(),
            tiles: prefab.tiles.len(),
            entities: prefab.entities,
            preview: Preview::of(&prefab.cells),
        });
        listed.push(Listed { name: joined(title), group: group.to_owned(), title: title.to_owned(), shape });
    }
}

/// A file's name without the prefab extension, if it has it, in any letter case.
fn strip_extension(file_name: &str) -> Option<&str> {
    let dot = file_name.len().checked_sub(EXTENSION.len() + 1)?;
    let extension = file_name.get(dot..)?;
    let stem = &file_name[..dot];
    (extension.starts_with('.') && extension[1..].eq_ignore_ascii_case(EXTENSION) && !stem.is_empty()).then_some(stem)
}

/// Where in a list the prefabs are whose name has every word of a search in it, in any letter
/// case, in the order of the list.
pub fn matching(listed: &[Listed], search: &str) -> Vec<usize> {
    let words: Vec<String> = search.split_whitespace().map(str::to_lowercase).collect();
    let found = |prefab: &Listed| {
        let name = prefab.name.to_lowercase();
        words.iter().all(|word| name.contains(word.as_str()))
    };
    listed.iter().enumerate().filter(|(_, prefab)| found(prefab)).map(|(index, _)| index).collect()
}

/// A name as the user typed it made into a prefab's name: `\` read as `/`, the parts trimmed,
/// empty parts dropped, and the extension taken off if it was typed. A name with no part, a
/// part that is `.` or `..`, or a character Windows does not allow in a file's name is refused.
pub fn clean_name(typed: &str) -> Result<String, String> {
    let typed = typed.trim().replace('\\', "/");
    let typed = strip_extension(&typed).unwrap_or(&typed).to_owned();
    let parts: Vec<&str> = typed.split('/').map(str::trim).filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return Err("give it a name".to_owned());
    }
    for part in &parts {
        if *part == "." || *part == ".." {
            return Err("part of the name is . or ..".to_owned());
        }
        if part.chars().any(|c| c.is_control() || "<>:\"|?*".contains(c)) {
            return Err(format!("\"{part}\" has a character that can't be in a file name"));
        }
        if part.ends_with('.') {
            return Err(format!("\"{part}\" ends with a dot"));
        }
    }
    Ok(parts.join("/"))
}

/// Where the file of a prefab of a name is.
fn path_of(folder: &Path, name: &str) -> PathBuf {
    let mut path = folder.to_path_buf();
    let mut parts = name.split('/').peekable();
    while let Some(part) = parts.next() {
        // The extension goes after the whole of the last part, which may have a dot of its own.
        if parts.peek().is_some() {
            path.push(part);
        } else {
            path.push(format!("{part}.{EXTENSION}"));
        }
    }
    path
}

/// Whether a prefab of a name, cleaned, is in the folder.
pub fn exists(folder: &Path, name: &str) -> bool {
    path_of(folder, name).exists()
}

/// Writes a prefab into the folder under a name the user typed, with the subfolders its name
/// has. A prefab of that name already there is kept and the save refused. The name it was
/// saved under comes back.
pub fn save(folder: &Path, typed: &str, prefab: &Prefab) -> Result<String, String> {
    let name = clean_name(typed)?;
    write_new(folder, &name, prefab.write().as_bytes())?;
    Ok(name)
}

/// Writes the bytes of a prefab file under a cleaned name, unless one is there.
fn write_new(folder: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let path = path_of(folder, name);
    if path.exists() {
        return Err(format!("a prefab named {name} already exists"));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&path, bytes).map_err(|error| error.to_string())
}

/// Reads the prefab of a name from the folder.
pub fn load(folder: &Path, name: &str) -> Result<Prefab, String> {
    let bytes = fs::read(path_of(folder, name)).map_err(|error| error.to_string())?;
    Prefab::read(&bytes)
}

/// Deletes the prefab of a name from the folder, and the subfolders it leaves empty.
pub fn delete(folder: &Path, name: &str) -> Result<(), String> {
    let path = path_of(folder, name);
    fs::remove_file(&path).map_err(|error| error.to_string())?;
    let mut parent = path.parent();
    while let Some(dir) = parent.filter(|dir| *dir != folder && dir.starts_with(folder)) {
        if fs::remove_dir(dir).is_err() {
            break;
        }
        parent = dir.parent();
    }
    Ok(())
}

/// Copies a prefab file into the top of the folder, under its own name. A file that cannot be
/// read as a prefab is refused, and so is one whose name is taken. The name it has in the
/// folder comes back.
pub fn import(folder: &Path, from: &Path) -> Result<String, String> {
    let bytes = fs::read(from).map_err(|error| error.to_string())?;
    Prefab::read(&bytes)?;
    let file_name = from.file_name().and_then(|name| name.to_str()).ok_or("the path doesn't point to a file")?;
    let name = clean_name(strip_extension(file_name).unwrap_or(file_name))?.replace('/', "");
    write_new(folder, &name, &bytes)?;
    Ok(name)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A folder of its own under the system's temporary folder, empty.
    pub(crate) fn temp_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("ss14-editor-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// A prefab of three cells, two tiles and a decal, with text that has every kind of line end.
    fn prefab() -> Prefab {
        Prefab {
            from: (-4, 7),
            roots: 2,
            entities: 3,
            cells: vec![(0, 0), (0, 1), (1, 0)],
            tiles: vec![
                CopiedTile { cell: (0, 0), tile: "FloorSteel".into(), variant: 2, rotation_mirroring: 5 },
                CopiedTile { cell: (1, 0), tile: "FloorWood".into(), variant: 0, rotation_mirroring: 0 },
            ],
            decals: vec![Decal { id: 7, prototype: "Arrows".into(), x: 0.25, y: 1.1, color: Some("#FF0000FF".into()), angle: 1.5707963267948966, z_index: -1, cleanable: true }],
            text: "\nmeta:\r\n  format: 7\n- uid: 1\n  name: \"caf\u{e9} \\\"x\\\"\"\r\n\n".into(),
        }
    }

    #[test]
    fn a_prefab_goes_to_a_file_and_back_the_same_with_its_text_byte_for_byte() {
        let prefab = prefab();
        let file = prefab.write();
        // The header is the first line, and the text all of the rest.
        let (header, text) = file.split_once('\n').unwrap();
        assert!(header.starts_with("{\"format\":\"ss14-editor-prefab\",\"version\":1,"));
        assert_eq!(text.as_bytes(), prefab.text.as_bytes());
        assert_eq!(Prefab::read(file.as_bytes()), Ok(prefab.clone()));

        // With no entities there is no text.
        let bare = Prefab { text: String::new(), entities: 0, roots: 0, ..prefab };
        assert_eq!(Prefab::read(bare.write().as_bytes()), Ok(bare));
    }

    #[test]
    fn a_broken_or_newer_file_is_refused_with_a_message() {
        let file = prefab().write();
        let newer = file.replacen("\"version\":1", "\"version\":2", 1);
        assert_eq!(Prefab::read(newer.as_bytes()), Err("it was made by a newer editor (format 2, this editor reads up to 1)".into()));
        let refused = |file: &str| Prefab::read(file.as_bytes()).unwrap_err();
        assert_eq!(refused(""), "it isn't a prefab file, or it's damaged");
        assert_eq!(refused("hello\nworld"), "it isn't a prefab file, or it's damaged");
        assert_eq!(refused(&file[..40]), "it isn't a prefab file, or it's damaged");
        assert_eq!(refused("{\"format\":\"something\",\"version\":1}\n"), "it isn't a prefab file from this editor");
        assert_eq!(refused("{\"format\":\"ss14-editor-prefab\"}\n"), "its header has no version");
        assert!(refused("{\"format\":\"ss14-editor-prefab\",\"version\":1,\"cells\":3}\n").starts_with("its header is broken"));
        assert_eq!(refused(&file.replacen("\"cells\":[[0,0],[0,1],[1,0]]", "\"cells\":[]", 1)), "it has no cells");
        assert_eq!(refused(&file.replacen("\"cells\":[[0,0],[0,1],[1,0]]", "\"cells\":[[0,0],[0,1]]", 1)), "it has a tile outside its cells");
        let mut bytes = file.into_bytes();
        bytes.push(0xFF);
        assert_eq!(Prefab::read(&bytes), Err("its entity text isn't UTF-8".into()));
    }

    #[test]
    fn names_are_cleaned_and_bad_ones_refused() {
        assert_eq!(clean_name("  rooms\\ kitchen /big.ss14prefab "), Ok("rooms/kitchen/big".into()));
        assert_eq!(clean_name("/a//b/"), Ok("a/b".into()));
        assert_eq!(path_of(Path::new("f"), "a/v1.2"), Path::new("f").join("a").join("v1.2.ss14prefab"));
        assert!(clean_name("  / ").is_err());
        assert!(clean_name("../up").is_err());
        assert!(clean_name("a:b").is_err());
        assert!(clean_name("what?").is_err());
        assert!(clean_name("dot.").is_err());
    }

    #[test]
    fn the_folder_lists_its_prefabs_with_subfolders_as_groups_and_is_searched() {
        let folder = temp_folder("list");
        for name in ["Zeta", "rooms/kitchen", "rooms/Bar", "Alpha", "shuttles/small/pod"] {
            save(&folder, name, &prefab()).unwrap();
        }
        fs::write(folder.join("broken.ss14prefab"), "nope").unwrap();
        fs::write(folder.join("notes.txt"), "not a prefab").unwrap();
        let listed = list(&folder);
        let names: Vec<(&str, &str, &str)> = listed.iter().map(|prefab| (prefab.name.as_str(), prefab.group.as_str(), prefab.title.as_str())).collect();
        assert_eq!(
            names,
            [
                ("Alpha", "", "Alpha"),
                ("broken", "", "broken"),
                ("Zeta", "", "Zeta"),
                ("rooms/Bar", "rooms", "Bar"),
                ("rooms/kitchen", "rooms", "kitchen"),
                ("shuttles/small/pod", "shuttles/small", "pod"),
            ]
        );
        assert_eq!(listed[1].shape, Err("it isn't a prefab file, or it's damaged".into()));
        let shape = listed[0].shape.as_ref().unwrap();
        assert_eq!((shape.size, shape.tiles, shape.entities), ((2, 2), 2, 3));
        // The top row has the cell (0, 1) alone.
        assert_eq!(shape.preview, Preview { width: 2, height: 2, filled: vec![true, false, true, true] });

        let found = |search: &str| matching(&listed, search).iter().map(|&index| listed[index].name.clone()).collect::<Vec<_>>();
        assert_eq!(found("ROOMS"), ["rooms/Bar", "rooms/kitchen"]);
        assert_eq!(found("rooms kit"), ["rooms/kitchen"]);
        assert_eq!(found("  "), names.iter().map(|name| name.0).collect::<Vec<_>>());
        assert!(found("nothing").is_empty());

        // A folder that is not there has none.
        assert!(list(&folder.join("missing")).is_empty());
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_save_goes_into_its_subfolder_and_keeps_what_is_there() {
        let folder = temp_folder("save");
        assert_eq!(save(&folder, "rooms\\kitchen", &prefab()), Ok("rooms/kitchen".into()));
        let path = folder.join("rooms").join("kitchen.ss14prefab");
        assert_eq!(fs::read_to_string(&path).unwrap(), prefab().write());
        assert_eq!(load(&folder, "rooms/kitchen"), Ok(prefab()));
        assert!(exists(&folder, "rooms/kitchen"));

        // A second of the same name is refused and the first kept.
        let other = Prefab { entities: 9, ..prefab() };
        assert_eq!(save(&folder, "rooms/kitchen", &other), Err("a prefab named rooms/kitchen already exists".into()));
        assert_eq!(load(&folder, "rooms/kitchen"), Ok(prefab()));
        assert!(save(&folder, "..", &other).is_err());
        assert!(load(&folder, "missing").is_err());
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_delete_takes_the_file_and_the_subfolders_it_empties() {
        let folder = temp_folder("delete");
        save(&folder, "a/b/one", &prefab()).unwrap();
        save(&folder, "a/two", &prefab()).unwrap();
        delete(&folder, "a/b/one").unwrap();
        assert!(!folder.join("a").join("b").exists());
        assert!(folder.join("a").join("two.ss14prefab").exists());
        delete(&folder, "a/two").unwrap();
        assert!(!folder.join("a").exists() && folder.exists());
        assert!(delete(&folder, "a/two").is_err());
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn an_import_copies_a_good_file_in_and_refuses_a_bad_one() {
        let folder = temp_folder("import");
        let outside = temp_folder("import-from");
        let good = outside.join("Pod.ss14prefab");
        fs::write(&good, prefab().write()).unwrap();
        assert_eq!(import(&folder, &good), Ok("Pod".into()));
        assert_eq!(fs::read(folder.join("Pod.ss14prefab")).unwrap(), fs::read(&good).unwrap());
        assert_eq!(import(&folder, &good), Err("a prefab named Pod already exists".into()));

        let bad = outside.join("bad.ss14prefab");
        fs::write(&bad, "{}").unwrap();
        assert_eq!(import(&folder, &bad), Err("it isn't a prefab file from this editor".into()));
        assert!(import(&folder, &outside.join("missing.ss14prefab")).is_err());
        assert_eq!(list(&folder).len(), 1);
        fs::remove_dir_all(&folder).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }
}
