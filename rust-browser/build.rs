fn main() {
    println!("cargo:rerun-if-changed=assets/aurora.ico");
    println!("cargo:rerun-if-changed=assets/aurora.manifest");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resources = winres::WindowsResource::new();
        resources.set_icon("assets/aurora.ico");
        // rfd's common-controls-v6 backend imports TaskDialogIndirect. Without
        // this activation context Windows selects comctl32 v5 before main.
        resources.set_manifest(include_str!("assets/aurora.manifest"));
        resources.set("ProductName", "Aurora Browser");
        resources.set("FileDescription", "Aurora privacy-first browser");
        resources
            .compile()
            .expect("embed Aurora icon into the Windows executable");
    }
}
