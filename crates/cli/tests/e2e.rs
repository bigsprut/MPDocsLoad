//! E2E-тесты CLI через assert_cmd (спец. §2.12 — E2E через CLI).

use assert_cmd::Command;

#[test]
fn providers_list_outputs_all_three() {
    let mut cmd = Command::cargo_bin("mdwf").unwrap();
    cmd.args(["providers", "list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("ozon"))
        .stdout(predicates::str::contains("wildberries"))
        .stdout(predicates::str::contains("test"));
}

#[test]
fn reports_list_ozon_has_realization() {
    let mut cmd = Command::cargo_bin("mdwf").unwrap();
    cmd.args(["reports", "list", "--provider", "ozon"])
        .assert()
        .success()
        .stdout(predicates::str::contains("ozon.realization"));
}

#[test]
fn reports_list_unknown_provider_errors() {
    let mut cmd = Command::cargo_bin("mdwf").unwrap();
    cmd.args(["reports", "list", "--provider", "nonexistent"])
        .assert()
        .failure();
}

#[test]
fn out_of_scope_wildberries_lists_3() {
    let mut cmd = Command::cargo_bin("mdwf").unwrap();
    cmd.args(["out-of-scope", "--provider", "wildberries"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Акты сверки"))
        .stdout(predicates::str::contains("Счета на оплату"))
        .stdout(predicates::str::contains("Договоры"));
}

#[test]
fn doctor_shows_config_and_providers() {
    let mut cmd = Command::cargo_bin("mdwf").unwrap();
    cmd.args(["doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MDWF диагностика"))
        .stdout(predicates::str::contains("Провайдеры"));
}

#[test]
fn schedule_list_runs_successfully() {
    // Команда должна выполняться успешно независимо от состояния БД:
    // выводит либо "Расписаний нет.", либо таблицу с заголовком.
    let mut cmd = Command::cargo_bin("mdwf").unwrap();
    let output = cmd.args(["schedule", "list"]).assert().success().get_output().clone();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Расписаний нет") || stdout.contains("Cron") || stdout.contains("Имя"),
        "unexpected schedule list output: {stdout}"
    );
}

#[test]
fn reports_info_period_report() {
    let mut cmd = Command::cargo_bin("mdwf").unwrap();
    cmd.args(["reports", "info", "ozon", "ozon.realization"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Period"))
        .stdout(predicates::str::contains("Отчёт о реализации"));
}
