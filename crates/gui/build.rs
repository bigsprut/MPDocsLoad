//! Build script:
//! 1. Компилирует gresource-бандл (SVG-иконки маркетплейсов) в бинарный
//!    `compiled.gresource` в `OUT_DIR` (регистрируется в main.rs).
//! 2. На Windows встраивает `resources/app-icon.ico` в exe как Win32-ресурс
//!    (иконка в проводнике/таскбаре/ярлыках) через winres → windres.

fn main() {
    glib_build_tools::compile_resources(
        &["resources"],
        "resources/resources.gresource.xml",
        "compiled.gresource",
    );

    // Перестраивать при изменении иконки.
    println!("cargo:rerun-if-changed=resources/app-icon.ico");
    println!("cargo:rerun-if-changed=resources/app-icon.svg");

    // Win32-ресурс с иконкой (только Windows; windres должен быть в PATH —
    // обеспечивается scripts/env.sh / build-release.sh).
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("resources/app-icon.ico");
        res.set("FileDescription", "MDWF — Marketplace Downloader Framework");
        res.set("ProductName", "MDWF");
        res.set("LegalCopyright", "© MDWF");
        if let Err(e) = res.compile() {
            // Не ломаем сборку, если windres недоступен — только warn.
            println!("cargo:warning=winres: не встроить иконку ({e})");
        }
    }
}
