const AutomationCore = (() => {
  function normalize(value) {
    return String(value == null ? "" : value)
      .normalize("NFD")
      .replace(/[\u0300-\u036f]/g, "")
      .trim()
      .toUpperCase()
      .replace(/\s+/g, "_");
  }

  function getSheetOrFinding_(spreadsheet, spec, findings) {
    const sheet = spreadsheet.getSheetByName(spec.name);
    if (!sheet && spec.required) {
      findings.push({severity: "CRITICAL", code: "MISSING_SHEET", sheet: spec.name});
    }
    return sheet;
  }

  function headerMap_(sheet, headerRow) {
    const width = Math.max(sheet.getLastColumn(), 1);
    const values = sheet.getRange(headerRow, 1, 1, width).getDisplayValues()[0];
    return values.reduce((map, header, index) => {
      const key = normalize(header);
      if (key) map[key] = index;
      return map;
    }, {});
  }

  function readTable_(sheet, spec) {
    const map = headerMap_(sheet, spec.headerRow);
    const lastRow = sheet.getLastRow();
    const width = Math.max(sheet.getLastColumn(), 1);
    const count = Math.max(lastRow - spec.headerRow, 0);
    return {
      map,
      rows: count ? sheet.getRange(spec.headerRow + 1, 1, count, width).getValues() : [],
      formulas: count ? sheet.getRange(spec.headerRow + 1, 1, count, width).getFormulas() : []
    };
  }

  function preflight() {
    const spreadsheet = SpreadsheetApp.getActive();
    const findings = [];
    const metrics = {sheetsExpected: 0, sheetsFound: 0, rowsScanned: 0};

    AUTOMATION_CONFIG.sheets.forEach(spec => {
      if (spec.required) metrics.sheetsExpected += 1;
      const sheet = getSheetOrFinding_(spreadsheet, spec, findings);
      if (!sheet) return;
      metrics.sheetsFound += 1;
      const table = readTable_(sheet, spec);
      metrics.rowsScanned += table.rows.length;

      (spec.requiredHeaders || []).forEach(header => {
        if (table.map[normalize(header)] == null) {
          findings.push({severity: "CRITICAL", code: "MISSING_HEADER", sheet: spec.name, header});
        }
      });

      (spec.idHeaders || []).forEach(header => {
        const column = table.map[normalize(header)];
        if (column == null) return;
        const seen = {};
        table.rows.forEach((row, offset) => {
          const id = String(row[column] == null ? "" : row[column]).trim();
          if (!id) return;
          if (seen[id]) {
            findings.push({
              severity: "HIGH",
              code: "DUPLICATE_ID",
              sheet: spec.name,
              header,
              id,
              rows: [seen[id], spec.headerRow + 1 + offset]
            });
          } else {
            seen[id] = spec.headerRow + 1 + offset;
          }
        });
      });

      (spec.formulaHeaders || []).forEach(header => {
        const column = table.map[normalize(header)];
        if (column == null) return;
        table.rows.forEach((row, offset) => {
          const hasValue = row.some(value => value !== "" && value != null);
          if (hasValue && !table.formulas[offset][column]) {
            findings.push({
              severity: "HIGH",
              code: "FORMULA_REPLACED",
              sheet: spec.name,
              header,
              row: spec.headerRow + 1 + offset
            });
          }
        });
      });
    });

    applyReferenceRules_(spreadsheet, findings);
    applyAcceptedValueRules_(spreadsheet, findings);
    if (typeof runDomainChecks === "function") runDomainChecks(spreadsheet, findings, metrics);
    return finalizeReport_("SYSTEM_PREFLIGHT", findings, metrics);
  }

  function applyReferenceRules_(spreadsheet, findings) {
    (AUTOMATION_CONFIG.rules.references || []).forEach(rule => {
      const fromSpec = AUTOMATION_CONFIG.sheets.find(item => item.name === rule.fromSheet);
      const toSpec = AUTOMATION_CONFIG.sheets.find(item => item.name === rule.toSheet);
      const fromSheet = spreadsheet.getSheetByName(rule.fromSheet);
      const toSheet = spreadsheet.getSheetByName(rule.toSheet);
      if (!fromSpec || !toSpec || !fromSheet || !toSheet) return;
      const from = readTable_(fromSheet, fromSpec);
      const to = readTable_(toSheet, toSpec);
      const fromColumn = from.map[normalize(rule.fromHeader)];
      const toColumn = to.map[normalize(rule.toHeader)];
      if (fromColumn == null || toColumn == null) return;
      const parents = new Set(to.rows.map(row => String(row[toColumn] || "").trim()).filter(Boolean));
      const ignored = new Set((rule.ignoreValues || []).map(value => String(value).trim()));
      from.rows.forEach((row, offset) => {
        const value = String(row[fromColumn] || "").trim();
        if (!value && rule.allowBlank) return;
        if (ignored.has(value)) return;
        if (value && !parents.has(value)) {
          findings.push({
            severity: "HIGH",
            code: "ORPHAN_REFERENCE",
            sheet: rule.fromSheet,
            header: rule.fromHeader,
            value,
            row: fromSpec.headerRow + 1 + offset
          });
        }
      });
    });
  }

  function applyAcceptedValueRules_(spreadsheet, findings) {
    (AUTOMATION_CONFIG.rules.acceptedValues || []).forEach(rule => {
      const spec = AUTOMATION_CONFIG.sheets.find(item => item.name === rule.sheet);
      const sheet = spreadsheet.getSheetByName(rule.sheet);
      if (!spec || !sheet) return;
      const table = readTable_(sheet, spec);
      const column = table.map[normalize(rule.header)];
      if (column == null) return;
      const accepted = new Set(rule.values.map(normalize));
      table.rows.forEach((row, offset) => {
        const raw = row[column];
        if (raw === "" || raw == null) return;
        if (!accepted.has(normalize(raw))) {
          findings.push({
            severity: "MEDIUM",
            code: "INVALID_CONTROLLED_VALUE",
            sheet: rule.sheet,
            header: rule.header,
            value: raw,
            row: spec.headerRow + 1 + offset
          });
        }
      });
    });
  }

  function validateProtectedOutcomes() {
    const spreadsheet = SpreadsheetApp.getActive();
    const findings = [];
    const metrics = {};
    if (typeof runProtectedOutcomeChecks === "function") {
      runProtectedOutcomeChecks(spreadsheet, findings, metrics);
    }
    return finalizeReport_("PROTECTED_OUTCOMES", findings, metrics);
  }

  function finalizeReport_(functionName, findings, metrics) {
    const critical = findings.filter(item => item.severity === "CRITICAL").length;
    const high = findings.filter(item => item.severity === "HIGH").length;
    return {
      schemaVersion: "1.0",
      runId: Utilities.getUuid(),
      timestamp: new Date().toISOString(),
      mode: "DRY_RUN",
      function: functionName,
      status: critical ? "BLOCKED" : high ? "REVIEW" : "PASS",
      metrics,
      findings
    };
  }

  function rememberReport(report) {
    if (report.mode === "DRY_RUN") return;
    PropertiesService.getDocumentProperties()
      .setProperty("LAST_AUTOMATION_REPORT", JSON.stringify(report).slice(0, 8000));
  }

  function showReport(report) {
    const summary = [
      `Estado: ${report.status}`,
      `Modo: ${report.mode || "DRY_RUN"}`,
      `Hallazgos: ${(report.findings || []).length}`,
      `Run: ${report.runId || "n/a"}`
    ].join("\n");
    SpreadsheetApp.getUi().alert(
      AUTOMATION_CONFIG.menuName,
      summary,
      SpreadsheetApp.getUi().ButtonSet.OK
    );
  }

  function appendAudit(functionName, mode, delta, status, details) {
    if (mode === "DRY_RUN") return;
    const spreadsheet = SpreadsheetApp.getActive();
    const lock = LockService.getDocumentLock();
    lock.waitLock(10000);
    try {
      const sheetName = AUTOMATION_CONFIG.auditSheet;
      const sheet = spreadsheet.getSheetByName(sheetName) || spreadsheet.insertSheet(sheetName);
      if (sheet.getLastRow() === 0) {
        sheet.appendRow([
          "Timestamp", "Run_ID", "Mode", "Function", "Actor",
          "Created", "Updated", "Conflicts", "Status", "Details_JSON"
        ]);
      }
      sheet.appendRow([
        new Date(),
        delta.runId || Utilities.getUuid(),
        mode,
        functionName,
        Session.getActiveUser().getEmail() || "UNKNOWN",
        delta.created || 0,
        delta.updated || 0,
        delta.conflicts || 0,
        status,
        JSON.stringify(details || {})
      ]);
    } finally {
      lock.releaseLock();
    }
  }

  return {
    normalize,
    preflight,
    validateProtectedOutcomes,
    rememberReport,
    showReport,
    appendAudit
  };
})();
