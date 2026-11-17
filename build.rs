fn main() {
    slint_build::compile("ui/app.slint").expect("compile the Slint UI");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("recursos/rustpira.ico")
            .set("ProductName", "Rustpira")
            .set("FileDescription", "Rustpira - recordatorio de descansos");
        resource.compile().expect("embed the Windows resources");
    }
}
