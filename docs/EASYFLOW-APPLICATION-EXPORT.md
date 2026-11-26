# EasyFlow 申請資料準備（2026-11-19）

## 本次交付

單據詳情增加「準備 EasyFlow 申請資料」。讀取已儲存的校對值、已對應案場及原始附件，預覽欄位後下載 CSV 或 JSON。
這是 B12 申請內容準備的第一段，不是 EasyFlow 連線、填入、送出或核准功能。
不需要地主租約即可產生草稿；沒有更改既有送審／租金計算／五人任一人通過的邏輯。

來源：11/19 提供的 `easy flow欄位對照圖.pptx`（7 頁）及本次討論。
付款與開票畫面為不同案場範例，不能把範例選項套用全部單據。客戶原始檔不隨本分支提交。

## 操作

1. 使用既有案場清單匯入功能建立主檔，完成電號對應；校對單據後先存檔。
2. 打開單據詳情，點「準備 EasyFlow 申請資料」。
3. 黃色欄位表示缺值或規則尚未設定。仍可下載，缺值為 null，絕不補零。
4. CSV 是供 Excel 閱讀的欄位／值／來源表，不改客戶原有公版。JSON 是自動化工具的結構化輸入，包含附件清單與待確認事項。
5. JSON 的 `submissionAllowed=false` 永遠保留。下游可做填表預覽，但不得把「可下載」當作「已核准可送出」。

## API

`GET /api/v1/orgs/{orgId}/energy/bills/{billId}/easyflow-application`

沿用平台 session 與 FLOW_VIEW 權限、組織隔離。`?format=csv` 可下載 CSV，預設 JSON。
本次沒有新增 API key 或外部服務。附件 `downloadPath` 仍需以有權限的 session 呼叫原下載 API，並非永久公開網址。

JSON：`schemaVersion=easyflow.application.v1`，包含 `billId`、`organizationId`、`generatedAt`、`applicationType`、
`reviewStatus`、`approvalState`、`fields[]`（key / label / value / status / source）、`lineItems[]`、
`lineItemsReconciled`、`attachments[]`（原檔名、hash、下載路徑）、`pending[]`。
金額一律 decimal 字串，保留零與負數，避免二進位浮點或缺值補零。

## 已實作的映射

|來源|用途|
|---|---|
|已對應案場 `data.plantNo`，其次 `project_code`|案場編號；不截取 PDF 檔名前四碼，不依賴手寫 OCR|
|案場 name / spv_payee_id → name|案場名稱、公司名稱|
|案場 data.invoice.counterpartyName / counterpartyTaxId|開票／付款對象與對象統編|
|renewableSurplus 的 purchaseEnergyFee|開票未稅金額，不使用扣除租費後的 totalAmount|
|付款類單據 abs(totalAmount)|付款總金額，同時保留原始 totalAmount|
|meterRentalFee / usageEnergyFee|付款明細分列，若無法與總額對平則標記，不猜差額|
|billingPeriodEnd / currentReadingDate|分別保留；紀錄表迄日使用哪個仍待確認|
|source_attachment_id|只附這張單據的原始附件，不混入同批其他案場文件|

分類使用平台有效單別：renewableSurplus → invoice；renewableNoSurplus、consumptionDevice、consumptionDeviceRental、consumptionBill → payment。
舊 electricityBill、未分類等不從正負號猜分支，輸出 unconfirmed。

## 案場匯入與設定

沿用既有 `/imports/stations` 的「發電案場清單」15 欄格式，**沒有宣稱可直接匯入任意超強表格工作表**。
本次增加保存「發票開立單的收款對象」「統一編號」到案場 data.invoice。舊資料若沒有這兩欄，請先用既有 dry-run 檢查，再重匯正確主檔或補資料；不依模糊名稱猜對象。

案場詳情頁「地主分配」下方有「EasyFlow 預設值」卡（卡 20261126-0214），對應 `GET`／`PUT /orgs/{orgId}/energy/stations/{stationId}/easyflow-defaults`，寫入 `data.easyflowDefaults`。
PUT 要有 `energy:master:manage` 權限，只收 9 個 key：companyName、companyCode、applicant、department、expectedReceiptDays、paymentMethod、paymentTerms、voucherStatus、contractOption。
字串上限 200 字；`expectedReceiptDays` 必須是 0～365 的整數（天數，不是固定日期）。其他 key 或值不合規回 400。
每次 PUT 整包取代（不是逐欄 patch），並寫一筆稽核 `station.easyflow_defaults_updated`。

草稿帶入規則：`companyName` 以案場綁定的 spv 受款對象名字優先，沒有才用 `easyflowDefaults.companyName` 備援；`applicationDate` 沒有存草稿、也沒有手填時用「今天」（台北時區）；開立類單據（invoice）且設定了 `expectedReceiptDays` 時，`expectedReceiptDate` = `applicationDate` + 天數。這兩個日期是在來源雜湊（`sourceRevision`）算完之後才補的，換日不會讓既有草稿變成 `needs_recheck`；已經存過的值不會被日期預設覆蓋。

**電費單的審核流程完成（`completed`）之後，EasyFlow 申請草稿不會被鎖成唯讀或 `needs_recheck`**：仍可重新存檔、重新確認，直到 `missingHandoffFields` 清空為止。只有原始金額等來源資料真的被改掉，草稿才會變成待重新核對（`stale`／`needs_recheck`），這跟流程本身有沒有跑完是兩件事。

## 本次驗證與未驗證邊界

- 回歸案例使用三爺二期數值：未稅 16874、稅 844、合計 17718、租費 -983、應付 16735；測試名稱使用合成公司，不提交客戶檔案。
- 測試涵蓋日期分離、零／缺值／錯誤數字、未知單別、付款明細對平、CSV 公式防護、跨組織／無權限拒絕、缺地主合約仍可輸出且不推進流程。
- 這次不重跑 OCR、不改辨識模型；測試從已儲存辨識／校對結果開始。
- EasyFlow 真實填入、附件上傳、送出與回填單號仍未驗證。原電費紀錄第 5090 列的完整自動寫入亦不在這個 endpoint 中。
- 租金規則與開票是否必須連動、付款選項、預計收款日及日期慣例仍待業務確認，並非匯出功能替代其核准。

## Rick 合併後

無 DB migration、無新增依賴。正常重建後端及前端即可，這次沒有部署客戶遠端。
先以本地／測試資料建立案場與單據，確認 CSV/JSON，再讓後續自動化工具依穩定 key 取值。

### 2026-11-19 本地驗證紀錄

通過：`cargo test -p eo-domain --test easyflow`（4）、`cargo test --test all energy_easyflow`（1，含 CSV、組織隔離及不推進流程）、
`cargo test --test all station_import`（1，含收款對象及統編保存）、`npm test`（204）、`npm run build`、`git diff --check`。
API 整合使用獨立測試庫、Postgres 18 與既有離線 MinIO 映像，沒有連客戶遠端。

擴大執行 `cargo test -p eo-domain` 時，既有 `audit_events::declares_only_types_that_are_written` 失敗：
事件 registry 比測試寫死的 56 項清單多了新事件。本分支未改這兩個檔案，留給 Rick 另案同步；因此**沒有宣稱完整 gate 全綠**。
前端既有路由數測試原先仍寫 80，但 master 已有 108；這次加入 1 個新 endpoint，同步維護為 109，逐端點測試全部通過。
環境沒有 rustfmt，未執行 fmt；未進行 EasyFlow 真實登入／填入／送出，也未重跑整套瀏覽器 crawler。
