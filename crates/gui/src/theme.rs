//! Тема оформления: брендовый CSS + схема цвета (спец. §2.5.4).

use gtk4::gdk::Display;
use gtk4::CssProvider;
use libadwaita as adw;

/// Цветовая схема.
pub enum ColorScheme {
    System,
    Light,
    Dark,
}

/// Применяет брендовый CSS MDWF (спец. §2.5.4).
pub fn apply_brand_css() {
    let css = r"
        @define-color mdwf_primary #1a365d;
        @define-color mdwf_accent  #2b6cb0;
        @define-color mdwf_success #16a34a;
        @define-color mdwf_warning #d97706;
        @define-color mdwf_error   #dc2626;

        button.suggested-action {
            background-color: @mdwf_accent;
        }
        progressbar > trough > progress {
            background-color: @mdwf_accent;
        }
        .status-ok    { color: @mdwf_success; font-weight: bold; }
        .status-warn  { color: @mdwf_warning; font-weight: bold; }
        .status-error { color: @mdwf_error;   font-weight: bold; }
        .dim-label {
            color: alpha(@theme_fg_color, 0.55);
            font-size: 0.9em;
        }
        .doc-list-row {
            padding: 6px;
        }
    ";

    let provider = CssProvider::new();
    provider.load_from_data(css);

    if let Some(display) = Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Устанавливает цветовую схему (спец. §2.5.4).
pub fn set_color_scheme(scheme: ColorScheme) {
    let manager = adw::StyleManager::default();
    let adw_scheme = match scheme {
        ColorScheme::System => adw::ColorScheme::Default,
        ColorScheme::Light => adw::ColorScheme::ForceLight,
        ColorScheme::Dark => adw::ColorScheme::ForceDark,
    };
    manager.set_color_scheme(adw_scheme);
}
