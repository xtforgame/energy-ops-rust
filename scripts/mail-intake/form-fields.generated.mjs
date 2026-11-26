// web/src/admin/domain-logic/components/energyOps/paymentFormFields.ts
var paymentFormSections = [["companyName", "\u516C\u53F8\u540D\u7A31\uFF08\u8CA0\u8CAC\u4ED8\u6B3E\u516C\u53F8\uFF09"], ["contractOption", "\u6709\u7121\u5408\u7D04"], ["summary", "\u7533\u8ACB\u6458\u8981"], ["counterpartyName", "\u4ED8\u6B3E\u5C0D\u8C61"], ["paymentMethod", "\u4ED8\u6B3E\u65B9\u5F0F"], ["paymentTerms", "\u4ED8\u6B3E\u689D\u4EF6"], ["voucherStatus", "\u6191\u8B49\u55AE\u64DA"]];
var esc = (v) => String(v ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
function paymentFormHtml(data) {
  if (data.applicationType !== "payment") return "";
  const val = (k) => data.fields.find((f) => f.key === k)?.value;
  const rows = paymentFormSections.map(([k, label], i) => `<tr><th>${i + 1}. ${label}</th><td>${esc(val(k)) || '<span class="missing">\u5F85\u88DC</span>'}</td></tr>`).join("");
  const lines = data.paymentLineItems ?? data.lineItems ?? [];
  return `<h2>\u96FB\u8CBB\u4ED8\u6B3E\u7533\u8ACB\uFF5C\u516B\u9805\u586B\u5BEB\u5167\u5BB9</h2><table>${rows}</table><h3>8. \u7533\u8ACB\u4E8B\u7531\u3001\u5E63\u5225\u3001\u91D1\u984D\uFF08\u5404\u9805\u5206\u5225\u586B\u5BEB\uFF09</h3><table><tr><th>\u7533\u8ACB\u4E8B\u7531</th><th>\u5E63\u5225</th><th>\u91D1\u984D</th></tr>${lines.map((l) => `<tr><td>${esc(l.description)}</td><td>${esc(l.currency ?? "TWD")}</td><td>${esc(l.amount)}</td></tr>`).join("") || '<tr><td colspan="3">\u8CBB\u7528\u5206\u9805\u5F85\u6838\u5C0D\uFF0C\u8ACB\u898B\u4E0B\u65B9\u539F\u55AE\u7D30\u9805\u3002</td></tr>'}<tr><th colspan="2">\u4ED8\u6B3E\u7533\u8ACB\u7E3D\u91D1\u984D\uFF08TWD\uFF09</th><td>${esc(val("amount"))}</td></tr></table><p>\u5206\u9805\u4F9D\u5DF2\u6838\u5C0D\u7684\u539F\u55AE\u91D1\u984D\u5F59\u6574\uFF1B\u539F\u59CB\u8A08\u8CBB\u7D30\u9805\u4FDD\u7559\u65BC\u4E0B\u65B9\uFF0C\u71DF\u696D\u7A05\u4E0D\u91CD\u8907\u52A0\u5165\u3002</p><h2>\u5176\u4ED6\u8CC7\u6599\u8207\u539F\u55AE\u5C0D\u7167</h2>`;
}

// web/src/admin/domain-logic/components/energyOps/rentalFormFields.ts
var rentalLabels = { companyName: "1. \u516C\u53F8\u540D\u7A31\uFF08\u8CA0\u8CAC\u4ED8\u6B3E\u516C\u53F8\uFF09", contractOption: "2. \u8CBB\u7528\u7A2E\u985E", summary: "3. \u7533\u8ACB\u6458\u8981", expenseDescription: "4. \u8CBB\u7528\u8AAA\u660E\uFF08\u516C\u5F0F\u8207\u4ED8\u6B3E\u65E5\u671F\u9650\u5236\uFF09", payee: "5. \u4ED8\u6B3E\u5C0D\u8C61\uFF08\u5EE0\u5546\uFF09", amount: "6. \u7E3D\u91D1\u984D", currency: "\u5E63\u5225", subtotal: "\u5C0F\u8A08", paymentMethod: "7. \u4ED8\u6B3E\u65B9\u5F0F", paymentDate: "8. \u8ACB\u5B89\u6392\u65BC\u6307\u5B9A\u6642\u9593\u4ED8\u6B3E", voucherStatus: "9. \u6191\u8B49\u55AE\u64DA", companyCode: "\u516C\u53F8\u4EE3\u78BC", department: "\u7533\u8ACB\u90E8\u9580", applicant: "\u7533\u8ACB\u4EBA", applicationDate: "\u7533\u8ACB\u65E5\u671F", paymentDeadline: "\u5408\u7D04\u7E73\u8CBB\u671F\u9650" };
var rentApplicationRows = (a) => Object.entries(rentalLabels).map(([k, label]) => [label, a.fields[k] ?? ""]);
var esc2 = (v) => String(v ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
function rentApplicationHtml(a) {
  const state = a.stale ? "\u8CC7\u6599\u66F4\u65B0\u5F8C\u5F85\u91CD\u65B0\u78BA\u8A8D" : a.state === "confirmed_for_handoff" ? "\u5167\u5BB9\u5DF2\u78BA\u8A8D\uFF0C\u5F85\u4EA4\u63A5" : a.state === "dataConfirmed" ? "\u8CC7\u6599\u5DF2\u8986\u6838\uFF0C\u5C1A\u672A\u9001\u51FA" : "\u8349\u7A3F\u5F85\u8986\u6838";
  return `<h2>EasyFlow \u79DF\u91D1\u4ED8\u6B3E\u7533\u8ACB\u55AE</h2><p>${state}\uFF1B\u5C1A\u672A\u9001\u51FA EasyFlow\u3001\u672A\u57F7\u884C\u4ED8\u6B3E\u3002</p><table>${rentApplicationRows(a).map(([k, v]) => `<tr${v ? "" : ' style="background:#fff5d7"'}><th>${esc2(k)}</th><td>${esc2(v)}</td></tr>`).join("")}</table><h3>\u4EA4\u63A5\u524D\u5F85\u88DC</h3><p>${esc2((a.missingFields ?? []).map((k) => rentalLabels[k] ?? k).join("\u3001") || "\u5DF2\u77E5\u5FC5\u586B\u6B04\u4F4D\u5DF2\u586B\uFF1B\u4ECD\u9808\u6838\u5C0D\u5BE6\u969B EF \u756B\u9762\u3002")}</p>`;
}

// web/src/admin/domain-logic/components/energyOps/rentPreviewFields.ts
function rentReferenceFields(r) {
  const fields = [];
  if (r.readiness === "notApplicable" || r.rentRequired === false) fields.push(["\u79DF\u91D1\u9069\u7528\u72C0\u614B", "\u4E0D\u9700\u4ED8\u79DF\u91D1\uFF1B\u4E0D\u5F71\u97FF\u96FB\u8CBB\u6D41\u7A0B"]);
  const components = r.paymentComponents ?? r.knownInputs?.paymentComponents ?? [];
  const missing = r.sourceMissing ?? r.knownInputs?.sourceMissing ?? [];
  if (components.length) {
    fields.push(["\u4ED8\u6B3E\u7D44\u6210\u8AAA\u660E", "\u4EE5\u4E0B\u70BA\u4F86\u6E90\u53C3\u8003\uFF0C\u672A\u5408\u8A08\u70BA\u6B63\u5F0F\u4ED8\u6B3E\uFF1B\u6B77\u53F2\u91D1\u984D\u4E0D\u8996\u70BA\u56FA\u5B9A\u79DF\u91D1"]);
    components.forEach((c, i) => {
      const title = `\u4ED8\u6B3E\u7D44\u6210 ${i + 1}\uFF5C${c.payee ?? "\u6536\u6B3E\u4EBA\u5F85\u78BA\u8A8D"}`;
      const detail = [c.cycleMonths != null ? `\u6BCF ${c.cycleMonths} \u500B\u6708\u4E00\u671F` : "\u9031\u671F\u5F85\u78BA\u8A8D"];
      if (c.annualAmountReference != null) detail.push(`\u5E74\u4ED8\u4F86\u6E90\u53C3\u8003\u91D1\u984D ${c.annualAmountReference} \u5143`);
      if (c.observedAmounts?.length) detail.push(`\u6B77\u53F2\u4ED8\u6B3E\u89C0\u5BDF\u503C ${c.observedAmounts.join("\u3001")} \u5143\uFF08\u4E0D\u662F\u56FA\u5B9A\u6708\u79DF\uFF09`);
      if (c.cycleMonths === 1 && c.fixedAmount == null) {
        if (missing.includes("monthlyComponentFormula")) detail.push("\u6BCF\u6708\u6B3E\u8A08\u7B97\u516C\u5F0F\u5F85\u78BA\u8A8D");
        if (missing.includes("monthlyComponentBasis")) detail.push("\u6BCF\u6708\u6B3E\u8A08\u7B97\u57FA\u5E95\u5F85\u78BA\u8A8D");
      }
      if (missing.includes("paymentDeductions")) detail.push("\u4ED8\u6B3E\u6263\u9805\u5F85\u78BA\u8A8D");
      if (c.sourceSheet) detail.push(`\u4F86\u6E90\uFF1A${c.sourceSheet}${c.sourceRow != null ? ` \u7B2C ${c.sourceRow} \u5217` : ""}`);
      fields.push([title, detail.join("\uFF1B")]);
    });
  }
  if (r.groupReferenceRent != null) {
    fields.push(["\u5171\u7528\u79DF\u91D1\uFF08\u6574\u7D44\uFF0C\u53EA\u8A08\u4E00\u6B21\uFF09", r.groupReferenceRent], ["\u5171\u7528\u91D1\u984D\u8AAA\u660E", "\u5C1A\u672A\u63D0\u4F9B\u500B\u5225\u5206\u6524\uFF1B\u6574\u7D44\u91D1\u984D\u53EA\u8A08\u4E00\u6B21\uFF0C\u4E0D\u5217\u70BA\u672C\u6848\u5834\u61C9\u4ED8"]);
    if (r.sharedGroupLabel) fields.push(["\u5171\u7528\u7FA4\u7D44", r.sharedGroupLabel]);
    if (r.sharedGroupKey) fields.push(["\u5171\u7528\u7FA4\u7D44\u8B58\u5225", r.sharedGroupKey]);
    if (r.sharedGroupMembers?.length) fields.push(["\u5171\u7528\u6848\u5834", r.sharedGroupMembers.join("\u3001")]);
  }
  if (r.annualRentReference != null && r.periodAllocationUnknown) {
    fields.push(["\u5DF2\u77E5\u5E74\u79DF\u91D1", r.annualRentReference], ["\u5206\u671F\u8AAA\u660E", "\u5206\u671F\u7B97\u6CD5\u5F85\u78BA\u8A8D\uFF1B\u4E0D\u63A8\u5B9A\u5E73\u5747\u5206\u671F\uFF0C\u53EF\u8F38\u5165 12 \u500B\u6708\u6838\u5C0D\u5E74\u984D"]);
  }
  return fields;
}
function rentCalculationDescription(r) {
  if (r.calculationType === "notApplicable" && r.rentRequired === false) return "\u696D\u4E3B\u81EA\u5EFA\u6848\uFF0C\u4E0D\u9700\u4ED8\u79DF\u91D1\uFF1B\u4E0D\u5F71\u97FF\u96FB\u8CBB\u6D41\u7A0B";
  const basis = r.basis === "gross" ? "\u542B\u7A05\u552E\u96FB\u6536\u5165" : r.basis === "untaxed" ? "\u672A\u7A05\u552E\u96FB\u6536\u5165" : "\u6536\u5165\u57FA\u5E95\u5F85\u78BA\u8A8D";
  const formula = r.formulaSummary || (r.calculationType === "annualReference" ? `\u552E\u96FB\u6536\u5165 \xD7 ${r.percent ?? "\u5F85\u78BA\u8A8D"}%\uFF1D\u53C3\u8003\u5E74\u79DF\u91D1 ${r.referenceRent ?? "\u5F85\u78BA\u8A8D"} \u5143\uFF1B\u5DEE\u8DDD\u672A\u9054 ${r.tolerancePercent ?? "\u5F85\u78BA\u8A8D"}% \u7DAD\u6301\u5951\u7D04\u5E74\u79DF\u91D1 ${r.annualRent ?? "\u5F85\u78BA\u8A8D"} \u5143\uFF1B\u9054\u9580\u6ABB\u6642\u53E6\u6838\u5C0D\u7D1A\u8DDD\uFF0C\u672C\u9810\u89BD\u4E0D\u63A8\u5B9A\u3002` : `\u6BD4\u4F8B ${r.percent ?? "\u5F85\u78BA\u8A8D"}${r.percent == null ? "" : "%"}`);
  const unit = r.minimumUnit === "month" ? "\u6BCF\u6708" : r.minimumUnit === "year" ? "\u6BCF\u5E74" : r.minimumUnit === "cycle" ? "\u6BCF\u671F" : "\u671F\u9593\u5F85\u78BA\u8A8D";
  const floor = r.calculationType === "percentageFloor" ? `\uFF1B\u6700\u4F4E\u984D ${r.minimumRent ?? "\u5F85\u78BA\u8A8D"} \u5143\uFF0F${unit}\uFF08${r.minimumBasis === "gross" ? "\u542B\u7A05" : r.minimumBasis === "untaxed" ? "\u672A\u7A05" : "\u7A05\u57FA\u5F85\u78BA\u8A8D"}\uFF09` : "";
  const rounding = r.rounding === "perReceipt" ? "\u5404\u7B46\u79DF\u91D1\u56DB\u6368\u4E94\u5165\u5F8C\u52A0\u7E3D" : r.rounding === "total" ? "\u8A08\u7B97\u79DF\u91D1\u7E3D\u984D\u5F8C\u56DB\u6368\u4E94\u5165" : "\u9032\u4F4D\u65B9\u5F0F\u5F85\u78BA\u8A8D";
  return `${formula}${floor}\uFF1B${basis}\uFF1B${rounding}\uFF1B${r.taxMode === "included" ? "\u5167\u542B" : "\u5916\u52A0"}\u7A05\u7387 ${r.taxPercent ?? "\u5F85\u78BA\u8A8D"}${r.taxPercent == null ? "" : "%"}\uFF1B\u5408\u7D04\u7E73\u8CBB\u671F\u9650 ${r.paymentDeadline || "\u5F85\u78BA\u8A8D"}\u3002`;
}
function rentPreviewFields(r) {
  const period = r.periodStart && r.periodEnd ? `${r.periodStart} \uFF5E ${r.periodEnd}` : "\u5F85\u78BA\u8A8D";
  const summary = r.application?.fields.summary?.trim() || `\u652F\u4ED8${r.stationName || "\u6848\u5834\u5F85\u6307\u5B9A"}\u79DF\u91D1\uFF08${period === "\u5F85\u78BA\u8A8D" ? "\u8A08\u7B97\u671F\u9593\u5F85\u78BA\u8A8D" : period}\uFF09`;
  return [["\u6848\u5834", r.stationName], ["\u96FB\u865F", r.customerNumber], ["\u6536\u6B3E\u5C0D\u8C61", r.payee], ["\u7533\u8ACB\u6458\u8981", summary], ["\u8A08\u7B97\u671F\u9593", period], ["\u6536\u5165\u57FA\u5E95", r.basis === "gross" ? "\u542B\u7A05\u552E\u96FB\u6536\u5165" : r.basis === "untaxed" ? "\u672A\u7A05\u552E\u96FB\u6536\u5165" : "\u6536\u5165\u57FA\u5E95\u5F85\u78BA\u8A8D"], ["\u6536\u5165\u5408\u8A08", r.income], ...rentReferenceFields(r), ...r.calculationType === "annualReference" ? [["\u53C3\u8003\u5E74\u79DF\u91D1", r.referenceRent], ["\u5951\u7D04\u7D93\u71DF\u5E74\u79DF\u91D1", r.annualRent], ["\u53C3\u8003\u5E74\u79DF\u91D1\u6BD4\u4F8B", `${r.referenceRatioPercent}%`]] : [], ...r.baseRentRaw != null ? [["\u79DF\u91D1\u57FA\u984D\uFF08\u672A\u9032\u4F4D\uFF09", r.baseRentRaw]] : [], ["\u79DF\u91D1\u57FA\u984D\uFF08\u5916\u52A0\u7A05\u524D\uFF09", r.baseRent], ...r.taxRaw != null ? [[r.taxMode === "included" ? "\u5167\u542B\u7A05\u984D\uFF08\u672A\u9032\u4F4D\uFF09" : "\u7A05\u984D\uFF08\u672A\u9032\u4F4D\uFF09", r.taxRaw]] : [], ...r.grossRentRaw != null ? [["\u542B\u7A05\u8A66\u7B97\u5408\u8A08\uFF08\u672A\u9032\u4F4D\uFF09", r.grossRentRaw]] : [], [r.taxMode === "included" ? "\u5167\u542B\u7A05\u984D" : "\u7A05\u984D", r.tax], ["\u61C9\u4ED8\u79DF\u91D1\u7E3D\u984D", r.grossRent], ["\u8CBB\u7528\u8AAA\u660E", rentCalculationDescription(r)], ["\u5408\u7D04\u7E73\u8CBB\u671F\u9650", r.paymentDeadline || "\u5F85\u78BA\u8A8D"], ["\u5B89\u6392\u4ED8\u6B3E\u65E5", r.application?.fields.paymentDate || "\u5F85\u4F5C\u696D\u78BA\u8A8D"], ["\u4ED8\u6B3E\u55AE\u865F", "\u5C1A\u672A\u7522\u751F"]];
}

// web/src/admin/domain-logic/components/energyOps/rentDocuments.ts
function rentDocumentChecklist(r, invoiceTiming) {
  const refs = (source) => {
    const rows = r.lines.filter((l) => l.source === source);
    return rows.length ? rows : (r.sourceContributions ?? []).filter((l) => l.source === source && l.attachmentId).map((l) => ({ source, attachmentId: l.attachmentId, page: l.page ?? 0, billId: l.billId }));
  };
  const sourceStatus = (source, empty) => {
    const rows = refs(source);
    if (!rows.length) return empty;
    const originals = rows.filter((l) => !!l.billId && l.attachmentId > 0).length;
    const others = rows.length - originals;
    if (originals) return `\u5DF2\u9023\u7D50 ${originals} \u7B46\u539F\u59CB\u96FB\u8CBB\u55AE\u9644\u4EF6${others ? `\uFF1B\u53E6\u6709 ${others} \u7B46\u4F86\u6E90\u5F85\u6838\u5C0D` : ""}\uFF1B\u8ACB\u6838\u5C0D\u671F\u9593\u8207\u5167\u5BB9`;
    return `\u5DF2\u5F15\u7528 ${rows.length} \u7B46${rows.some((l) => l.sourceRow) ? "\u8A66\u7B97\u8868\u7D00\u9304\uFF0C\u5C1A\u672A\u9010\u5F35\u6838\u5C0D\u539F\u55AE" : "\u4F86\u6E90\u9801\uFF1B\u8ACB\u6838\u5C0D\u671F\u9593"}`;
  };
  return [
    { key: "calculation", label: "\u79DF\u91D1\u8A08\u7B97\u8868", status: r.grossRent != null && !r.issues.length ? "\u5DF2\u7522\u51FA\u8A66\u7B97\u9810\u89BD\uFF08\u672A\u6838\u51C6\uFF09" : "\u4F86\u6E90\u6216\u8A08\u7B97\u5C1A\u5F85\u78BA\u8A8D", references: [] },
    { key: "contract", label: "\u5408\u7D04\u79DF\u91D1\u9801\u9762\u622A\u5716", status: r.evidenceStatus === "complete" ? "\u5B8C\u6574\u5408\u7D04\u5DF2\u5099\uFF08\u8ACB\u6838\u5C0D\u9644\u4EF6\uFF09" : r.evidenceStatus === "excerpt" ? "\u5DF2\u6709\u79DF\u91D1\u9801\u6458\u9304\uFF08\u5B8C\u6574\u5408\u7D04\u5F85\u88DC\uFF09" : "\u5F85\u9644\u5408\u7D04\u79DF\u91D1\u9801\u9762", references: (r.leaseEvidence?.attachmentIds ?? []).map((attachmentId) => ({ source: "lease", attachmentId, page: 0 })) },
    { key: "taipower", label: "\u53F0\u96FB\u96FB\u8CBB\u55AE", status: sourceStatus("taipower", "\u5C1A\u672A\u5F15\u7528\u53F0\u96FB\u4F86\u6E90"), references: refs("taipower") },
    { key: "transfer", label: "\u8F49\u4F9B\u96FB\u8CBB\u55AE", status: sourceStatus("transfer", "\u5C1A\u672A\u5F15\u7528\uFF1B\u5F85\u78BA\u8A8D\u662F\u5426\u9069\u7528"), references: refs("transfer") },
    { key: "invoice", label: "\u5730\u4E3B\u767C\u7968\uFF0F\u6536\u64DA", status: (r.application?.voucherAttachmentIds?.length ?? 0) > 0 ? `\u5DF2\u9023\u7D50 ${r.application.voucherAttachmentIds.length} \u4EFD\u6191\u8B49\uFF08\u8ACB\u6838\u5C0D\u5167\u5BB9\uFF09` : invoiceTiming === "later" ? "\u767C\u7968\u5F85\u5F8C\u88DC\uFF08\u5C1A\u672A\u6536\u5230\uFF09" : invoiceTiming === "before" ? "\u7533\u8ACB\u524D\u9700\u9644\u767C\u7968\uFF08\u5C1A\u672A\u78BA\u8A8D\u6536\u4EF6\uFF09" : "\u5148\u9644\uFF0F\u5F8C\u88DC\u5F85\u78BA\u8A8D", references: (r.application?.voucherAttachmentIds ?? []).map((attachmentId) => ({ source: "voucher", attachmentId, page: 0 })) }
  ];
}
var escapeRentHtml = (s) => String(s ?? "\u5F85\u78BA\u8A8D").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
function rentDocumentChecklistHtml(r, timing) {
  return "<h2>\u7533\u8ACB\u9644\u4EF6\u6E05\u55AE</h2><p>\u4EE5\u4E0B\u70BA\u6E96\u5099\u6E05\u55AE\uFF0C\u672A\u9001\u51FA\u7533\u8ACB\u3002\u55AE\u7368 HTML \u4E0D\u5167\u5D4C\u9644\u4EF6\uFF1BZIP \u662F\u5426\u5305\u542B\u6A94\u6848\u8ACB\u770B\u5305\u5167\u6E05\u55AE\u3002" + ((r.application?.draftRevision ?? 0) > 0 ? "\u7533\u8ACB\u8CC7\u6599\u8207\u6191\u8B49\u5B89\u6392\u5DF2\u4FDD\u5B58\u65BC\u5E73\u53F0\uFF1B" : "\u76EE\u524D\u70BA\u81EA\u52D5\u9810\u89BD\uFF0C\u7533\u8ACB\u8349\u7A3F\u5C1A\u672A\u4FDD\u5B58\uFF1B") + "\u5F8C\u88DC\u671F\u9650\u5C1A\u5F85\u78BA\u8A8D\uFF0C\u672A\u81EA\u52D5\u555F\u7528\u63D0\u9192\u3002</p><table><tr><th>\u6587\u4EF6</th><th>\u76EE\u524D\u72C0\u614B</th><th>\u4F86\u6E90\u9644\u4EF6\uFF0F\u9801\u78BC</th></tr>" + rentDocumentChecklist(r, timing).map((x) => `<tr><td>${escapeRentHtml(x.label)}</td><td>${escapeRentHtml(x.status)}</td><td>${x.references.map((l) => `${escapeRentHtml(l.attachmentId)}\uFF0F${escapeRentHtml(l.page || "\u672A\u6307\u5B9A\u9801\u78BC")}`).join("\u3001") || "\u2014"}</td></tr>`).join("") + "</table>";
}
export {
  paymentFormHtml,
  rentApplicationHtml,
  rentCalculationDescription,
  rentDocumentChecklistHtml,
  rentPreviewFields
};
