//! Форматирование вывода CLI.

use mdwf_core::Profile;

/// Печатает список провайдеров.
pub fn print_providers(list: &[mdwf_core::ProviderRef]) {
    println!("{:<15} {:<20} {:<10} {}", "ID", "Имя", "Версия", "Отчётов");
    println!("{}", "-".repeat(70));
    for p in list {
        println!(
            "{:<15} {:<20} {:<10} {}",
            p.id(),
            p.display_name(),
            p.version(),
            p.capabilities().reports.len()
        );
    }
}

/// Печатает список профилей.
pub fn print_profiles(list: &[Profile]) {
    println!("{:<20} {:<15} {}", "Имя", "Провайдер", "Описание");
    println!("{}", "-".repeat(60));
    for p in list {
        println!(
            "{:<20} {:<15} {}",
            p.name,
            p.provider_id,
            p.description.as_deref().unwrap_or("-")
        );
    }
}

/// Печатает список отчётов провайдера.
pub fn print_reports(provider_id: &str, list: &[mdwf_core::ReportDescriptor]) {
    println!("Отчёты провайдера '{provider_id}':\n");
    println!("{:<30} {:<10} {:<12} {}", "Тип", "Режим", "Категория", "Название");
    println!("{}", "-".repeat(85));
    for r in list {
        let mode = if r.acquisition_mode.is_browsable() {
            "Список"
        } else {
            "Период"
        };
        let cat = format!("{:?}", r.category).to_lowercase();
        println!("{:<30} {:<10} {:<12} {}", r.type_id, mode, cat, r.display_name);
    }
}

/// Печатает out-of-scope документы.
pub fn print_out_of_scope(provider: &str, docs: &[(&str, &str)]) {
    println!("Документы '{provider}', недоступные через API:\n");
    for (name, hint) in docs {
        println!("  • {name}");
        println!("    {hint}");
    }
}
