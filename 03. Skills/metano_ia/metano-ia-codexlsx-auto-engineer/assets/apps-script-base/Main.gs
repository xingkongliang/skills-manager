function onOpen(e) {
  SpreadsheetApp.getUi()
    .createMenu(AUTOMATION_CONFIG.menuName)
    .addItem("1 · Diagnóstico integral (DRY RUN)", "runSystemPreflight")
    .addItem("2 · Validar gates protegidos", "validateProtectedOutcomes")
    .addSeparator()
    .addItem("Ver último reporte", "showLastReport")
    .addToUi();
}

function runSystemPreflight() {
  const report = AutomationCore.preflight();
  AutomationCore.rememberReport(report);
  AutomationCore.showReport(report);
  return report;
}

function validateProtectedOutcomes() {
  const report = AutomationCore.validateProtectedOutcomes();
  AutomationCore.rememberReport(report);
  AutomationCore.showReport(report);
  return report;
}

function showLastReport() {
  const raw = PropertiesService.getDocumentProperties().getProperty("LAST_AUTOMATION_REPORT");
  const report = raw ? JSON.parse(raw) : {status: "NO_REPORT", findings: []};
  AutomationCore.showReport(report);
}
