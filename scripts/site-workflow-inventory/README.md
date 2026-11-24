# 案場 × 工作流盤點大表

一個指令從來源檔產生「案場 × 工作流」盤點的 xlsx，裡面三張表：「表一 代號」「表二 案場×工作流」
「出處」。只用 Python 標準函式庫（不裝 openpyxl）。真實資料（案場名、電號、金額、姓名、帳號、
專案編號）不進這個 repo，終端機只印筆數。

## 怎麼跑

真實的來源設定與對照檔放在 repo 外（例如盤點用的工作資料夾），在 energy-ops-rust 底下跑：

```
python3 -I scripts/site-workflow-inventory/build.py \
  --sources  /path/to/你的sources.json \
  --map-dir  /path/to/你的對照檔資料夾 \
  --out      /path/to/輸出/案場工作流盤點.xlsx
```

自查（合理測試）：

```
python3 -I scripts/site-workflow-inventory/check.py /path/to/輸出/案場工作流盤點.xlsx \
  --sources /path/to/你的sources.json
```

## 三個參數／環境變數

`--sources`、`--map-dir`、`--out` 都可以省略；省略時依序用環境變數 `SWI_SOURCES`／`SWI_MAP_DIR`／
`SWI_OUT`，再不然用這個資料夾底下的 `sources.json`、`對照檔/`、`out/案場工作流盤點.xlsx` 當預設值
（這幾個預設路徑都被 `.gitignore` 擋住，不會進 repo）。

- `--sources`：來源 xlsx／json 的路徑設定檔，格式照 `sources.example.json`（複製一份改成
  `sources.json`，換新版來源時只改這一份，不用改程式）。
- `--map-dir`：人工對照檔的資料夾，格式照 `對照檔範本/` 的四份 csv（複製一份出去、填真實內容；
  UTF-8 加 BOM，Excel 打得開、改得了）：
  - `代號表.csv`：13 個代號的定義，這份直接用 repo 裡的這一份（不是範本，內容本來就不含真實案場資料）。
  - `案場判斷.csv`：人工覆寫某個案場某一行（電費／租金）的代號、參數、確認狀態、下一步。
  - `名稱對照.csv`：電費單紀錄用的案場名稱 → 專案編號，補自動比對不到的。
  - `檔案對照.csv`：原始資料裡的租約、付款申請單、EF 截圖、電費單 PDF → 專案編號與類型。
- `--out`：輸出 xlsx 的路徑。

## 哪些檔不進這個 repo

這個資料夾的 `.gitignore` 擋：`out/`（產出的 xlsx）、`sources.json`（真實來源路徑，含內部檔案系統
路徑）、`對照檔/`（填了真實案場資料的對照檔）。repo 裡只有：`build.py`、`check.py`、`代號表.csv`
（代號定義，不含案場資料）、`sources.example.json`（佔位路徑）、`對照檔範本/`（三份只有表頭的 csv）、
這份 README。真實的 sources.json 與填好的對照檔放在 repo 外（例如盤點卡的工作資料夾），用
`--sources`／`--map-dir` 指過去跑。

## 已知限制

- 名稱對照：電費單紀錄用的「案場名稱」常常跟資產清單的名稱不一樣，自動比對不到的，表二「電費單」
  「發票或收據」欄會顯示「未對上（名稱對照.csv 沒有對應）」，不會跟真的「缺」混在一起；要精確，
  在 `名稱對照.csv` 補上對應。
- 電號共用（同一顆電表掛在多個專案上）：共用電號的每個專案都會標成「兩份都有」，表二「參數」欄
  附註「電號與另 N 個專案共用：<其他專案編號>」，提醒這幾個案場的電號、發電案場清單資料是共用
  同一份來源。
- 代號猜測：電費代號只照固定幾欄規則猜 E-A／E-B／E-C，猜不出 E-D（逆變器，無合約），要靠
  `案場判斷.csv` 人工填。地主類型（RP-1／RP-3／RP-4）是照地主姓名裡的關鍵字猜的，不保證準，
  標「推測」，真的要用請跟實際資料來源確認後填進 `案場判斷.csv`。
