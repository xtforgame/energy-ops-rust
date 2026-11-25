# 可攜租金資料包匯入

工具：`scripts/data-import/import-rent-package.mjs`，使用 Node 18+ 原生 API，無 npm 依賴。帳密由目標部署的 env 檔讀取，資料包不含登入資訊或本機資料庫／附件 ID。

```text
node scripts/data-import/import-rent-package.mjs --base BASE --org ORG --env-file ENV_FILE --package rent-data.json --out import-plan.json
node scripts/data-import/import-rent-package.mjs --base BASE --org ORG --env-file ENV_FILE --apply-plan import-plan.json --out applied.json
node scripts/data-import/import-rent-package.mjs --base BASE --org ORG --env-file ENV_FILE --package rent-data.json --out recheck.json
```

先備份目標資料庫。第一步只預檢；blocked 必須為0。第二步使用第一步固定計畫，逐筆確認版本、寫入並讀回。第三步應全部 unchanged。

電號與案名須唯一符合電費主表；規則用sourceKey對應。既有資料只套用本輪改動欄位，若同欄與原始／修正後皆不同則停止；目標其他設定及附件保留。無任何規則才新增，已有其他來源規則則停止，不重複新增。

規則建立API沒有跨請求的來源鍵唯一約束，因此匯入期間限一人操作，不要與其他匯入或人工建規則並行。每次POST前重查，但不能宣稱支援並發建立的去重。

不是整批資料庫交易。若中斷，重新預檢確認已完成資料，再套用新計畫，不直接重送未知結果的POST。計畫及套用報告保存更新前後資料；整批復原以事前備份為準。

資料包與大表含客戶資料，分開交付，不納入程式Git。第三分頁是缺項清單；Excel編輯不會直接更動匯入JSON。

驗證：9項Node測試涵蓋欄位合併、衝突、附件保護、唯一電號，以及模擬HTTP新增／更新／讀回／重跑。本地真實32筆已更新資料之預檢與套用均為32 unchanged、0新增、0更新；未在Rick端或客戶遠端執行。
