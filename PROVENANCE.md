# 規格來源

| 範圍 | repo | commit |
|---|---|---|
| 能源業務（App 5，Supabase 版） | `potted-dev-team/workspaces/5-2fv26mb84qh6` main（匯出在 az repo `temp/tenant-console-compare-20261104/3-online-energy-ops-app`） | `85631c6` |
| 能源原版與設計文件、組織／RBAC／登入 | az repo `feat-tenant-console-route-transition` | `0afe402aba` |

## 複製進來的 fixture

| 檔 | 來源 | sha256 |
|---|---|---|
| `tests/fixtures/canonical-samples.json` | App 5 `backend/tests/fixtures/canonical-samples.json` | 46dde82c091bf798981af3792fe0ee2ef4e59686177522d359b8b5f84b3b23f6 |
| `tests/fixtures/samples/電費單(有餘電需開發票).pdf` | az repo `feat-tenant-console-route-transition`（`0afe402aba`）`docs/features/energy-ops-console/各類電費單範本/` | bdb4305e73254df2c5e1bebb8a231623215eae5a8fa16f89ef58aaec6fbe75d7 |
| `tests/fixtures/samples/電費單(無餘電需付款).pdf` | 同上 | 388982ae35824bde66d9384fe29dc35c1df724a56b18c1c4051c939e179bee45 |
| `tests/fixtures/samples/逆變器電費單(僅設備發電使用電費).pdf` | 同上 | 2fea70b3e8a2f947f66cb2c28f128047c19cddb2cacaa99035e388fa095dd50c |
| `tests/fixtures/samples/逆變器電費單(含電表租費&設備發電使用電費).pdf` | 同上 | 037a2a7d3b170fa51b532992abcc8cb3f17bb54dc4c336014352be342b9c087e |

四份範本 PDF 給 e2e crawler（`web/e2e/crawlers/*`）上傳用；檔名與 `canonical-samples.json` 的鍵、假辨識服務（`fake-tpr`）認的名字相同，不要改名。
