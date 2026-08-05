//! Build script: компилирует gresource-бандл (SVG-иконки маркетплейсов)
//! в бинарный `compiled.gresource` в `OUT_DIR`. Регистрируется в `main.rs`
//! через `gio::resources_register_include!("compiled.gresource")`.

fn main() {
    glib_build_tools::compile_resources(
        &["resources"],
        "resources/resources.gresource.xml",
        "compiled.gresource",
    );
}
