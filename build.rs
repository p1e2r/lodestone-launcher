fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    // Embed the app icon into the Windows .exe.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Lodestone");
        res.set("FileDescription", "Lodestone Minecraft launcher");
        if let Err(e) = res.compile() {
            println!("cargo:warning=couldn't embed the Windows icon: {e}");
        }
    }
}
