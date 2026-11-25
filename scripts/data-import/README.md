# 既有平台資料匯入（不重建資料庫）

## 已匯入規則補齊來源條件

`refresh-rent-rules.mjs` 用於補齊已存在規則，不另建重複紀錄。資料包每筆包含 `expectedVersion` 與完整新 `rule`；以唯一電號及來源 sourceKey 找既有規則，先核對全部版本與來源雜湊，才可套用。

```powershell
# 先產出含 before/after 的私有更新清單，不寫入
node refresh-rent-rules.mjs --base http://127.0.0.1:11951 --org 1 --package rules.json --env-file 平台私有.env --out planned.json
# 確認清單後執行；既有版本若已變更則拒絕覆寫
node refresh-rent-rules.mjs --base http://127.0.0.1:11951 --org 1 --package rules.json --env-file 平台私有.env --out applied.json --apply
```

公式可試算與確認紀錄分開。原表已載的比例／年租金可供試算；未提供進位方式時顯示未取整值，未知稅務不偽填 0%。共同地主保留群組及來源分配，不自動平均分攤。

## 2026-11-25：可重用租金規則

新增 migration `0111_station_rent_rules.sql`。案場電號仍沿用既有主檔；租金規則保存於 `tbl_energy_station_rent_rule`，修改版本留存於 revision 表。完整租約是否已附，與規則是否可用分開記錄。

`prepare-rent-rules.py` 讀取資產清單的租金欄位及案場對照，預設全部待確認。只有人工核對的 overrides 能標已確認；簽署日不會自動當作規則生效日。重複電號或無法唯一對應的列列入 unmatched。

```powershell
python prepare-rent-rules.py --asset 資產清單.xlsx --stations stations.json --overrides overrides.json --out prepared-rules.json
node import-rent-rules.mjs --base http://127.0.0.1:11951 --org 1 --package prepared-rules.json --env-file 平台私有.env --out import-report.json
```

先備份目標資料庫。匯入器使用 API 與管理員權限，按目標環境的唯一主電號重新找案場，不直接沿用來源 stationId。相同 sourceKey 已存在時保留平台版本，避免覆蓋使用者修改。不同來源需人工確認，不能直接當作新規則啟用。

已確認且支援的比例規則會依週期讀取案場電費單最終欄位產出租金預覽。缺當期單據、來源重複、規則涵蓋不完整或待確認時顯示原因；完整租約附件缺漏不阻擋試算。歷史人工草稿保留，重疊期間不混算。其他計算類型先保存條件待確認，不宣稱已支援自動計算。

真實來源檔、overrides 與 prepared-rules 都放在 Git 外。此工具不代表自動部署客戶資料庫，也不送出 EasyFlow。

若資料包含租約附件 ID，資料包必須另有 `attachmentScope: {base: "目標網址", organizationId: 組織ID}`；匯入器拒絕跨環境沿用。換環境須先重新上傳或依檔案雜湊找回該環境附件，再更新 ID 與 scope，不能只改 scope。

程式庫只保存工具。真實案場與地主資料、聯絡資訊、付款紀錄放在獨立資料包，不提交公開 Git。

## 資料包內容

- stations.csv/json：總表電號正規化後的唯一對應，含來源列號；電號保持字串及前導零。
- rent-rules.csv/json：資產清單的租金條款原值。個別已核對範例另外標示，未知公式不自動啟用。
- rent-payment-history.csv、electricity-history.csv：歷史原始列；不是新付款指令。
- source-rows.json：完整來源列與試算表公式／快取值。
- pending.csv/json：缺漏或來源間無法直接對應的資料，保留不猜配。
- manifest.json：來源 SHA256、筆數；databaseApplied=false 表示尚未匯入。
- data.sql：資料本體；與 import.sql 一起使用，不含本地 stationId / attachmentId。

## 在既有平台 PostgreSQL 匯入

先備份資料庫；平台需已執行既有 migrations（至少包含案場、電號與受款對象擴欄）。
把 import.sql 放在 data.sql 旁，使用能連到目標資料庫的 psql。連線資訊依部署環境設定，不寫進 Git。

```powershell
# 必須填正確組織 ID；先預演，預設也會 rollback。
psql "$env:EO_DATABASE_URL" -v org_id=2 -v apply=false -f .\import.sql
# 核對預演結果後再執行寫入。
psql "$env:EO_DATABASE_URL" -v org_id=2 -v apply=true -f .\import.sql
```

若使用容器內 psql，將 import.sql 與 data.sql 複製到同一容器目錄，再執行上述 -f 參數即可。組織 2 只是本地示例，不可照抄到另一環境。

### 寫入位置及界線

| 內容 | 目的表／欄位 | 行為 |
|---|---|---|
| 案場、電號 | tbl_energy_station、tbl_energy_station_meter | 依組織＋正規化電號／電廠編號比對；多重衝突跳過。不更改既有名稱、容量、地主等欄位 |
| 新案場所屬公司 | tbl_energy_landowner 的 subsidiary 類型 | 僅補不存在的公司；沿用平台匯入器必要主檔欄位，標記 landownerAssignmentPending，不產生地主付款分配 |
| 發票聯絡欄位 | station.data.invoice | 只補空值，不覆蓋已有值，不把「-」當有效資料 |
| 租金規則及來源 | station.data.sourceImports | 保存原值、來源、待確認與驗算範例；**目前不是自動計算引擎讀取的已啟用策略** |
| 原始列／歷史紀錄 | tbl_energy_source_import_row | 依組織＋檔案雜湊／分頁／列號去重，已存在不重複寫入 |

不覆寫 rentPaymentDrafts、rentCycleDraft、既有合約或策略。不建立付款、EF申請或審核結果。尚未對上案場的資料仍保存在來源表，供後續確認。

變更來源檔會有新雜湊，作為新版來源留存，不靜默覆寫舊版。與既有主檔有不同值時保留原主檔並保存來源，不能將匯入成功當作差異已解決。

後續接線：Rick 可由 sourceImports 中確認的條款，轉成現有合約／rentCycleDraft；須先確認生效期間及必要參數。範例 PDF 以雜湊＋頁碼識別，重新上傳後才取得該環境附件 ID，不能沿用另一台的數字 ID。

## 重建資料包與驗證

```powershell
python prepare.py --sources C:\資料來源 --out C:\匯入包 --verified-examples C:\已核對範例.json
python -m unittest discover -s . -p test_prepare.py
python test_sql.py
```

抽取需 Python openpyxl；已產出的資料包匯入只需 psql，不需 Python。test_sql.py 使用本地已存在 postgres:15-alpine，在獨立容器驗證去重、rollback、衝突跳過與組織隔離，不碰正式平台資料庫。驗證完自動移除測試容器。

2026-11-22：本地平台 WSL 連線失敗，依使用者指示不重啟。資料包已產出，但本地平台匯入及電費單端到端實跑尚未執行；獨立 SQL 測試不代表平台驗收。

同日後續：另建隔離 Docker 測試平台（未重啟原服務），已實際匯入 98 個案場及 98 筆電號，並以六份既有 PDF 驗證電號對應與 EasyFlow 待審表產出。這是本地測試資料庫，不代表客戶遠端已匯入；詳見 `docs/2026-11-22-pdf-to-easyflow-review.md`。
