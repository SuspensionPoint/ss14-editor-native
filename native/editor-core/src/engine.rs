//! The game engine, hosted inside this process.
//!
//! The .NET runtime is loaded the same way `dotnet Editor.Native.dll` would load it, but its
//! `Main` never runs. The engine is called through plain function pointers to `NativeExports`
//! (src/Editor.Native). Each call blocks until the engine has finished, so make them from a thread
//! that isn't drawing.

use std::path::Path;

use netcorehost::{nethost, pdcstr, pdcstring::PdCString};
use serde_json::Value;

type StartFn = extern "system" fn(*const u8, i32, *const u8, i32) -> i32;
type WaitReadyFn = extern "system" fn(*mut i32) -> *mut u8;
type OpenFileFn = extern "system" fn(*const u8, i32, *mut i32) -> *mut u8;
type PaintTileFn = extern "system" fn(*const TileEdit, *const u8, i32, *mut i32) -> *mut u8;
type SetLayersFn = extern "system" fn(i32, i32, *const i32, i32, *mut i32) -> *mut u8;
type CommandFn = extern "system" fn(*const u8, i32, *const u8, i32, *mut i32) -> *mut u8;
type CallFn = extern "system" fn(*const u8, i32, *const u8, i32, *mut i32) -> *mut u8;
type DrawListFn = extern "system" fn(i32, *mut *const u8) -> i32;
type SaveFn = extern "system" fn(i32, *mut i32) -> *mut u8;
type FreeFn = extern "system" fn(*mut u8);

/// Which tile to paint. The same layout as `NativeExports.TileEdit`, passed by address.
#[repr(C)]
pub struct TileEdit {
    pub document: i32,
    pub grid: i32,
    pub x: i32,
    pub y: i32,
}

/// Function pointers into the hosted engine. Cheap to copy and usable from any thread.
#[derive(Clone, Copy)]
pub struct Engine {
    start: StartFn,
    wait_ready: WaitReadyFn,
    open_file: OpenFileFn,
    paint_tile: PaintTileFn,
    set_layers: SetLayersFn,
    command: CommandFn,
    call: CallFn,
    draw_list: DrawListFn,
    save: SaveFn,
    free: FreeFn,
}

impl Engine {
    /// Loads the runtime and binds the engine's functions. `engine_folder` is the published
    /// src/Editor.Native: the folder with Editor.Native.dll in it.
    pub fn load(engine_folder: &Path) -> Result<Engine, String> {
        let hostfxr = nethost::load_hostfxr().map_err(|e| format!("no .NET runtime found: {e}"))?;
        let app = PdCString::from_os_str(engine_folder.join("Editor.Native.dll")).map_err(|e| e.to_string())?;
        let context = hostfxr
            .initialize_for_dotnet_command_line(&app)
            .map_err(|e| format!("couldn't prepare the runtime for {}: {e}", engine_folder.display()))?;
        let loader = context.get_delegate_loader().map_err(|e| e.to_string())?;
        let exports = pdcstr!("SS14.EditorNative.NativeExports, Editor.Native");

        macro_rules! managed {
            ($type:ty, $name:literal) => {
                *loader
                    .get_function_with_unmanaged_callers_only::<$type>(exports, pdcstr!($name))
                    .map_err(|e| format!("the engine has no function {}: {e}", $name))?
            };
        }
        let engine = Engine {
            start: managed!(StartFn, "Start"),
            wait_ready: managed!(WaitReadyFn, "WaitReady"),
            open_file: managed!(OpenFileFn, "OpenFile"),
            paint_tile: managed!(PaintTileFn, "PaintTile"),
            set_layers: managed!(SetLayersFn, "SetLayers"),
            command: managed!(CommandFn, "Command"),
            call: managed!(CallFn, "Call"),
            draw_list: managed!(DrawListFn, "DrawList"),
            save: managed!(SaveFn, "Save"),
            free: managed!(FreeFn, "Free"),
        };

        // The runtime lives as long as the process does; the context that started it is only a
        // handle, and closing it while managed threads run gains nothing.
        std::mem::forget(context);
        Ok(engine)
    }

    fn reply(&self, text: *mut u8, length: i32) -> Result<Value, String> {
        let bytes = unsafe { std::slice::from_raw_parts(text, length as usize) };
        let value: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        (self.free)(text);
        if value["ok"] == true {
            Ok(value["result"].clone())
        } else {
            Err(value["error"].as_str().unwrap_or("the engine reported a failure").to_owned())
        }
    }

    /// Starts both kernels and returns at once.
    pub fn start(&self, fork_repo: &str, kernel_pack: Option<&str>) {
        let pack = kernel_pack.unwrap_or("");
        (self.start)(fork_repo.as_ptr(), fork_repo.len() as i32, pack.as_ptr(), pack.len() as i32);
    }

    /// Waits until both kernels are up.
    pub fn wait_ready(&self) -> Result<Value, String> {
        let mut length = 0;
        let text = (self.wait_ready)(&mut length);
        self.reply(text, length)
    }

    /// Opens a map file and waits until its draw list is recorded.
    pub fn open_file(&self, path: &str) -> Result<Value, String> {
        let mut length = 0;
        let text = (self.open_file)(path.as_ptr(), path.len() as i32, &mut length);
        self.reply(text, length)
    }

    /// Paints one tile and waits until the changed draw list is recorded.
    pub fn paint_tile(&self, edit: &TileEdit, tile: &str) -> Result<Value, String> {
        let mut length = 0;
        let text = (self.paint_tile)(edit, tile.as_ptr(), tile.len() as i32, &mut length);
        self.reply(text, length)
    }

    /// The layer switches: flags are 1 entities, 2 markers, 4 decals, 8 what lies under floors;
    /// `hidden_depths` is pairs of lowest and highest draw depth to hide. Waits until the changed
    /// draw list is recorded.
    pub fn set_layers(&self, document: i32, flags: i32, hidden_depths: &[i32]) -> Result<Value, String> {
        let mut length = 0;
        let text = (self.set_layers)(document, flags, hidden_depths.as_ptr(), hidden_depths.len() as i32, &mut length);
        self.reply(text, length)
    }

    /// One of the document kernel's named commands (KernelApi.cs), JSON in and out.
    pub fn command(&self, op: &str, args: &str) -> Result<Value, String> {
        let mut length = 0;
        let text = (self.command)(op.as_ptr(), op.len() as i32, args.as_ptr(), args.len() as i32, &mut length);
        let bytes = unsafe { std::slice::from_raw_parts(text, length as usize) };
        let value: Result<Value, String> = serde_json::from_slice(bytes).map_err(|e| e.to_string());
        (self.free)(text);
        value
    }

    /// One of the editor's commands (src/Editor.Native/NativeApi.cs), with its answer read
    /// straight into the type the caller wants. Where the command changes how a document
    /// looks, its draw list is recorded by the time this returns.
    pub fn call<T: serde::de::DeserializeOwned>(&self, op: &str, args: &Value) -> Result<T, String> {
        #[derive(serde::Deserialize)]
        struct Reply<T> {
            ok: bool,
            result: Option<T>,
            error: Option<String>,
        }
        let args = args.to_string();
        let mut length = 0;
        let text = (self.call)(op.as_ptr(), op.len() as i32, args.as_ptr(), args.len() as i32, &mut length);
        let bytes = unsafe { std::slice::from_raw_parts(text, length as usize) };
        let reply: Result<Reply<T>, String> = serde_json::from_slice(bytes).map_err(|e| format!("couldn't read the engine's answer to '{op}': {e}"));
        (self.free)(text);
        let reply = reply?;
        if !reply.ok {
            return Err(reply.error.unwrap_or_else(|| "the engine reported a failure".into()));
        }
        reply.result.ok_or_else(|| format!("the engine gave no answer to '{op}'"))
    }

    /// Hands the latest draw-list message of a document to `read`, where the engine wrote it.
    /// The memory is only good until the engine records the next message for that document, so
    /// it must not be kept.
    pub fn with_draw_list<R>(&self, document: i32, read: impl FnOnce(&[u8]) -> R) -> Option<R> {
        let mut address: *const u8 = std::ptr::null();
        let length = (self.draw_list)(document, &mut address);
        if length <= 0 || address.is_null() {
            return None;
        }
        Some(read(unsafe { std::slice::from_raw_parts(address, length as usize) }))
    }

    pub fn save(&self, document: i32) -> Result<Value, String> {
        let mut length = 0;
        let text = (self.save)(document, &mut length);
        self.reply(text, length)
    }
}
