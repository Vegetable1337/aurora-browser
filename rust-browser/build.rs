fn main() {
    println!("cargo:rerun-if-changed=assets/aurora.ico");
    println!("cargo:rerun-if-changed=assets/aurora.manifest");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // GUI applications should not open an attached console in release builds.
        // Keep test harness executables as console apps so the MSVC linker uses main().
        if std::env::var("PROFILE").as_deref() == Ok("release") {
            println!("cargo:rustc-link-arg-bin=aurora-browser=/SUBSYSTEM:WINDOWS");
        }
        let mut resources = winres::WindowsResource::new();
        resources.set_icon("assets/aurora.ico");
        // rfd's common-controls-v6 backend imports TaskDialogIndirect. Without
        // this activation context Windows selects comctl32 v5 before main.
        resources.set_manifest(include_str!("assets/aurora.manifest"));
        resources.set("ProductName", "Aurora Browser");
        resources.set("FileDescription", "Aurora privacy-first browser");
        resources.set("FileVersion", "0.1.0.0");
        resources.set("ProductVersion", "0.1.0-alpha");
        resources.set("OriginalFilename", "Aurora.exe");
        resources
            .compile()
            .expect("embed Aurora icon into the Windows executable");
    }
}
