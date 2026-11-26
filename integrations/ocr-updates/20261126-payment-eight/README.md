# OCR 更新：20261126-payment-eight

這是增量原始碼補丁，不是完整 OCR 安裝包，也不含模型或機密資料。適用現有 taipower-bill-extraction-demo；需已具備先前版本的選頁／拆單功能及依賴。

## 內容
- modernRenewableBillParser 保留原單「臨櫃繳納／匯款至」說明到 paymentNote，供平台填付款方式。
- 新增回歸測試；修正既有 processBill 測試的 optional-array TypeScript 錯誤。
- 不改 OCR 模型、帳號或資料庫。

## Rick 套用步驟（PowerShell）
先將 marrash 合併進你要部署的平台版本，再處理獨立的 OCR 專案。以下路徑換成你本機位置。

```powershell
$platformRepo = 'C:\你的路徑\energy-ops-rust'
$ocrRepo = 'C:\你的路徑\taipower-bill-extraction-demo'
$patchFile = Join-Path $platformRepo 'integrations\ocr-updates\20261126-payment-eight\ocr-payment-eight.patch'
Set-Location $ocrRepo
# 先備份或提交 OCR 目前的修改。只有 check 成功才套用。
git apply --check $patchFile
if ($LASTEXITCODE -ne 0) { throw '補丁版本不一致，請先比對 updated-files，不要覆蓋現有修改。' }
git apply $patchFile
if ($LASTEXITCODE -ne 0) { throw '套用失敗' }
npx vitest run tests/modernRenewableBillParser.test.ts
if ($LASTEXITCODE -ne 0) { throw '測試失敗' }
npm run build
if ($LASTEXITCODE -ne 0) { throw '建置失敗' }
```

如果已套用過，可用 `git apply --reverse --check $patchFile` 檢查，不要重複套用。updated-files 是修改後三份原始檔供人工比對；有其他改動時不要直接整檔覆蓋。

## 部署要一起更新
- 平台：重新建置 Rust 後端與 web 前端，使用合併後同一版本。
- OCR：將新 dist 編入現有 OCR 映像，或更新既有 dist 掛載目錄，再重建/重啟 OCR 服務。
- dist 更新後請重啟 OCR worker，避免既有瀏覽器載入舊靜態資源而逾時。本地曾遇到，重啟後同批次重試成功。
- 不需重建資料庫。Git 不會同步本地案場設定或既有帳單。
- 現有部署方式不同，不可直接用平台根目錄 docker-compose.yml 當作全服務更新命令（該檔只有資料庫與 MinIO）。請沿用你原本的平台/OCR部署設定。

## 更新後驗證
用原始PDF重新上傳，而非只重開舊單：
1. 無餘電需付款：132 元；匯款；電表租費發票 UJ28211985。
2. 逆變器含租費：使用費3221 + 租費1549 = 4770；無合約、自動扣款、無／後補。
3. 逆變器僅使用費：5035；無合約、自動扣款、無／後補。
4. 有餘電：保留開票申請，未稅127848。

前三份下載HTML最上方為八項付款內容；API paymentLineItems 與 CSV 的 EF申請分項一致，原單 lineItems 保留核對。申請人、部門、公司代碼等未設定資料不可宣稱齊全。未送 EF。

完整本地驗證記錄：docs/REVIEW-2026-11-26-PAYMENT-EIGHT.md。
