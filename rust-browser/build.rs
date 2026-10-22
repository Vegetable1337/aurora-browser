fn main() {
    println!("cargo:rerun-if-changed=assets/aurora.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resources = winres::WindowsResource::new();
        resources.set_icon("assets/aurora.ico");
        resources.set("ProductName", "Aurora Browser");
        resources.set("FileDescription", "Aurora privacy-first browser");
        resources
            .compile()
            .expect("embed Aurora icon into the Windows executable");
    }
}
