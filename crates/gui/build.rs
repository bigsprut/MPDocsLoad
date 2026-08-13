//! Build script:
//! 1. Компилирует gresource-бандл (иконки маркетплейсов) в бинарный
//!    `compiled.gresource` (регистрируется в main.rs через resources_register_include!).
//! 2. На Windows встраивает `resources/app-icon.ico` в exe как Win32-ресурс
//!    (иконка в проводнике/таскбаре/ярлыках) + VERSIONINFO.
//!
//! ВАЖНО про п.2: раньше использовался крейт `winres`, но его `resource.o` НЕ
//! линковался в exe — glib-build-tools эмитит `cargo:rustc-link-lib=static=resource`
//! (→ libresource.a), и winres-овский объект с тем же именем терялся. Итог: в exe
//! попадало только version-info (или ничего), а иконка — НЕТ → дефолтная иконка в
//! проводнике. Поэтому встраиваем вручную: .rc → windres → `cargo:rustc-link-arg`.

fn main() {
    glib_build_tools::compile_resources(
        &["resources"],
        "resources/resources.gresource.xml",
        "compiled.gresource",
    );

    println!("cargo:rerun-if-changed=resources/app-icon.ico");
    println!("cargo:rerun-if-changed=resources/app-icon.svg");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(target_os = "windows")]
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
        let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
        // АБСОЛЮТНЫЙ путь к иконке со слешами '/': windres запускается с cwd=OUT_DIR и
        // не найдёт относительный путь; обратные слэши в .rc — escape-символы.
        let icon_abs = std::path::Path::new(&manifest_dir)
            .join("resources")
            .join("app-icon.ico")
            .to_string_lossy()
            .replace('\\', "/");

        let rc_path = std::path::Path::new(&out_dir).join("mdwf_app.rc");
        let o_path = std::path::Path::new(&out_dir).join("mdwf_app.o");
        // .rc: иконка (RT_ICON) + VERSIONINFO. BEGIN/END — синтаксис windres.
        let rc = format!(
            "#pragma code_page(65001)\n\
             1 ICON \"{icon}\"\n\
             1 VERSIONINFO\n\
             FILEVERSION 1,4,0,0\n\
             PRODUCTVERSION 1,4,0,0\n\
             FILEOS 0x40004\n\
             FILETYPE 0x1\n\
             BEGIN\n\
               BLOCK \"StringFileInfo\"\n\
               BEGIN\n\
                 BLOCK \"000004b0\"\n\
                 BEGIN\n\
                   VALUE \"FileDescription\", \"MDWF — Marketplace Downloader Framework\"\n\
                   VALUE \"ProductName\", \"MDWF\"\n\
                   VALUE \"ProductVersion\", \"1.4.0\"\n\
                   VALUE \"LegalCopyright\", \"© MDWF\"\n\
                 END\n\
               END\n\
               BLOCK \"VarFileInfo\"\n\
               BEGIN\n\
                 VALUE \"Translation\", 0x0, 0x04b0\n\
               END\n\
             END\n",
            icon = icon_abs
        );
        if let Err(e) = std::fs::write(&rc_path, rc) {
            println!("cargo:warning=icon embed: не записать .rc ({e})");
        }
        // windres должен быть в PATH (scripts/env.sh / build-release.sh).
        let status = std::process::Command::new("windres")
            .arg(&rc_path)
            .args(["-O", "coff"])
            .arg("-o")
            .arg(&o_path)
            .status();
        match status {
            Ok(s) if s.success() => {
                // Прямой link-arg — гарантия, что .o с иконкой попадёт в exe.
                println!("cargo:rustc-link-arg={}", o_path.display());
            }
            _ => {
                println!("cargo:warning=icon embed: windres не смог скомпилировать .rc (он должен быть в PATH — scripts/env.sh)");
            }
        }
    }
}
