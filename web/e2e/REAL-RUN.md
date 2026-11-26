# 錄「真單全流程」怎麼重拍

`crawlers/record-real-run.ts` 讓員工在畫面上手動走一次全流程（案場→地主→合約→EasyFlow 預設值→上傳真單→
電號指認→審核→租金試算→付款草稿→付款申請），邊錄影邊截圖，用真實 API 回包核對每一步，不是只看畫面字樣。

## 1. 先準備設定檔（真資料，不進 repo）

複製範本，填真值：

```bash
cp e2e/real-run.example.json e2e/real-run.local.json
```

`e2e/real-run.local.json` 已經在 `.gitignore` 裡，`git status` 看不到它、也不會被 commit。鍵的說明看
`real-run.example.json` 裡的範本值與 `sources` 區塊的提示文字。

也可以不放在 `web/e2e/` 底下，改用環境變數 `E2E_REAL_RUN_CONFIG` 指到別的檔案路徑。

## 2. 環境變數

| 變數 | 說明 |
|---|---|
| `E2E_BASE_URL` | 後端 base URL，例如 `http://127.0.0.1:10923` |
| `E2E_SUPER_USER` / `E2E_SUPER_PASSWORD` | 登入帳密 |
| `E2E_ORG_ID` | 組織 id |
| `S3_OUT` | 輸出 mp4 路徑（截圖跟這個檔案存同一個資料夾） |
| `E2E_REAL_RUN_CONFIG` | 設定檔路徑；沒設就讀 `web/e2e/real-run.local.json` |
| `E2E_STOP_AFTER_SEG`（可選） | 只跑到某一段就停（例如 `3`），smoke 用 |

## 3. 先清空自己的本機測試環境（指令範本，庫名用變數，不要碰其他人的庫）

**一定要先清空再跑完整段0～9**：段1會用 API 核對 `stations`／`landowners`／`contracts` 都是 0 筆，
不是0筆就直接 throw 擋下來（不會偷偷在非空庫上繼續跑，新建的案場/地主只是 id 往後排、肉眼不容易看出庫不是空的）。
只跑 `E2E_STOP_AFTER_SEG=3` 的 smoke 可以不清空（純粹驗證腳本機制，不是驗證「從空系統建」這件事本身）。

```bash
DB_NAME=你自己的測試庫名   # 例如 eo_e2e_xxxx，不要用 energy_ops／其他人在用的庫
kill <你自己起的後端 PID>
docker compose exec -T postgres psql -U energy -d postgres -c "DROP DATABASE IF EXISTS ${DB_NAME};"
docker compose exec -T postgres psql -U energy -d postgres -c "CREATE DATABASE ${DB_NAME};"
# 重新起後端（環境變數照你自己那組：EO_DATABASE_URL 指到 $DB_NAME、EO_S3_BUCKET 用自己的 bucket 名）
# 開機 5 步：login → 建組織 → install-defaults → 把自己的帳號加進審核部門 ×2
```

清空之後，錄影前務必先用 API 確認 `/stations`／`/landowners`／`/contracts`／`/bills` 都是 `{"items":[]}`。

**不要用 `run-crawlers.sh`**——那支是跑既有的 e2e 測試合集，不是給這支錄影腳本用的。

## 4. 跑

```bash
cd web
E2E_BASE_URL=http://127.0.0.1:xxxxx \
E2E_SUPER_USER=你的帳號 \
E2E_SUPER_PASSWORD=你的密碼 \
E2E_ORG_ID=你的org id \
S3_OUT=/tmp/real-run.mp4 \
npx tsx e2e/crawlers/record-real-run.ts
```

先用 `E2E_STOP_AFTER_SEG=3` 跑一次 smoke（段0～3，確認登入、清空、建案場/地主基本功能正常），
畫面／log 看過沒問題，再清空環境、不帶這個變數跑一次完整段0～9。

## 5. 轉成 H.264（給 QuickTime 之類的播放器直接雙擊播，不是 vp9）

```bash
ffmpeg -i /tmp/real-run.mp4 -c:v libx264 -pix_fmt yuv420p -movflags +faststart /tmp/real-run-h264.mp4
```

轉完用 `ffprobe` 確認 `codec_name=h264`、`pix_fmt=yuv420p`。

## 6. 真資料別留在 repo 裡

commit 前先跑一次 `git diff master...HEAD` 加 `git diff --cached`，grep 你知道的真實字串（案場名、合約編號、
公司代碼、電號、人名、本機路徑……），確認 0 筆才 commit；`e2e/real-run.local.json` 本身已經被 `.gitignore`
擋住，但還是要檢查有沒有把真值貼進其他要 commit 的檔案裡（例如改錯地方貼進了 `.ts`／`.md`）。
