//! Gives the Windows .exe the app's icon (assets/icon.ico, the loading screen's clown).

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/icon.ico");
        // An icon is a nicety: a machine without the resource compiler still builds the editor.
        if let Err(error) = resource.compile() {
            println!("cargo:warning=the .exe was built without its icon: {error}");
        }
    }
}
