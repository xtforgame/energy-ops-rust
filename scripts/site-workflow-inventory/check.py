"""自查產出的 xlsx（合理測試）。

用法：python3 -I check.py <輸出.xlsx> [--sources <sources.json>]
驗：三張表都在；表二列數＝2×案場數（案場數用來源檔算出來，不寫死）、19 欄；
表一、表二每個有值的格子在出處表都有一列（用非空格數對照）；代號都在表一；
表二每格的 style（s）跟「組別單雙＋第 10～17 欄的值」算出來的一致（S6 上色）。
印 OK，或第一個錯（只印表名、欄名、列號，不印值）。
"""
import argparse
import pathlib
import sys
import zipfile
import xml.etree.ElementTree as ET

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import build  # noqa: E402

NS = build.NS
M = build.M


def _open(path):
    z = zipfile.ZipFile(path)
    ss = []
    if "xl/sharedStrings.xml" in z.namelist():
        for si in ET.fromstring(z.read("xl/sharedStrings.xml")).findall("m:si", NS):
            ss.append("".join(t.text or "" for t in si.iter(M + "t")))
    wb = ET.fromstring(z.read("xl/workbook.xml"))
    rels = ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))
    rmap = {r.get("Id"): r.get("Target") for r in rels}
    sheets = {}
    for sh in wb.find("m:sheets", NS):
        tgt = rmap[sh.get("{%s}id" % NS["r"])].lstrip("/")
        tgt = tgt if tgt.startswith("xl/") else "xl/" + tgt
        sheets[sh.get("name")] = build._read_sheet_rows(z, ss, tgt)
    return sheets


def fail(msg):
    print("錯：" + msg)
    sys.exit(1)


def _sheet_target(z, sheet_name):
    wb = ET.fromstring(z.read("xl/workbook.xml"))
    rels = ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))
    rmap = {r.get("Id"): r.get("Target") for r in rels}
    for sh in wb.find("m:sheets", NS):
        if sh.get("name") == sheet_name:
            tgt = rmap[sh.get("{%s}id" % NS["r"])].lstrip("/")
            return tgt if tgt.startswith("xl/") else "xl/" + tgt
    return None


def _read_sheet_with_style(z, tgt):
    """跟 build._read_sheet_rows 類似，但多帶每格的 s（style id）。"""
    root = ET.fromstring(z.read(tgt))
    rows = []
    for r in root.iter(M + "row"):
        cells = {}
        for c in r.findall(M + "c"):
            v = c.find(M + "v")
            is_ = c.find(M + "is")
            val = None
            if is_ is not None:
                val = "".join(t.text or "" for t in is_.iter(M + "t"))
            elif v is not None:
                val = v.text
            cells[build._col_num(c.get("r"))] = (val, int(c.get("s") or 0))
        rows.append((int(r.get("r")), cells))
    return rows


def check_t2_colors(path):
    """S6：表二每格的 s 跟 build.t2_cell_style(row_idx0, col, value) 算出來的要一樣。"""
    z = zipfile.ZipFile(path)
    tgt = _sheet_target(z, "表二 案場×工作流")
    if tgt is None:
        fail("表二 案場×工作流 找不到 sheet 位置，無法驗上色")
    for rn, cells in _read_sheet_with_style(z, tgt):
        if rn <= 1:
            continue
        row_idx0 = rn - 2
        for col in range(1, 20):
            val, s = cells.get(col, (None, None))
            if s is None:
                fail(f"表二 案場×工作流 第 {rn} 列第 {col} 欄沒有 s（空格也要帶 s）")
            expect = build.t2_cell_style(row_idx0, col, val)
            if s != expect:
                fail(f"表二 案場×工作流 第 {rn} 列第 {col} 欄 style 是 {s}，照規則應該是 {expect}")


def main(path, sources=None):
    if sources:
        build.configure(sources=sources)
    sheets = _open(path)
    for name in ("表一 代號", "表二 案場×工作流", "出處"):
        if name not in sheets:
            fail(f"缺 sheet {name!r}")

    t1 = sheets["表一 代號"]
    t1_data = [c for rn, c in t1 if rn > 1 and c]
    if len(t1_data) != 13:
        fail(f"表一 代號 列數應該是 13，實際 {len(t1_data)}")
    t1_cols = max((max(c) for rn, c in t1 if c), default=0)
    if t1_cols != 12:
        fail(f"表一 代號 欄數應該是 12，實際 {t1_cols}")

    t2 = sheets["表二 案場×工作流"]
    t2_data = [(rn, c) for rn, c in t2 if rn > 1 and c]
    t2_cols = max((max(c) for rn, c in t2 if c), default=0)
    if t2_cols != 19:
        fail(f"表二 案場×工作流 欄數應該是 19，實際 {t2_cols}")

    # 案場數用來源檔算出來，不寫死
    src = build.load_sources()
    sites = build.merge_sites(src, [])
    expect_rows = 2 * len(sites)
    if len(t2_data) != expect_rows:
        fail(f"表二 案場×工作流 列數應該是 2×案場數={expect_rows}（來源檔實測案場數 {len(sites)}），實際 {len(t2_data)}")

    # 表一、表二每個有值的格子，在出處表都算得到至少一列（用非空格數粗估，不逐格比對）
    prov = sheets["出處"]
    prov_data = [c for rn, c in prov if rn > 1 and c]
    t1_filled = sum(1 for rn, c in t1 if rn > 1 for v in c.values() if v not in (None, ""))
    t2_filled = sum(1 for rn, c in t2 if rn > 1 for v in c.values() if v not in (None, ""))
    if len(prov_data) < (t1_filled + t2_filled):
        fail(f"出處 列數 {len(prov_data)} 少於表一+表二有值的格子數 {t1_filled + t2_filled}")

    # 代號都在表一：表二第 6 欄代號（切分 ＋／、），排除「未判定」「RP未判定」，要出現在表一第 1 欄代號清單
    t1_codes = set()
    for rn, c in t1:
        if rn > 1 and c.get(1):
            t1_codes.add(c[1].strip())
    for rn, c in t2:
        if rn <= 1 or not c:
            continue
        code_val = (c.get(6) or "").strip()
        if not code_val or code_val == "未判定":
            continue
        for token in build._split_codes(code_val):
            if token == "RP未判定":
                continue
            if token not in t1_codes:
                fail(f"表二 案場×工作流 第 {rn} 列第 6 欄的代號 {token!r} 不在表一 代號 裡")

    check_t2_colors(path)

    print("OK")
    print(f"表一 {len(t1_data)} 列；表二 {len(t2_data)} 列（案場數 {len(sites)}）；出處 {len(prov_data)} 列")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("xlsx")
    ap.add_argument("--sources", help="sources.json 的路徑（傳給 build.load_sources）")
    a = ap.parse_args()
    main(a.xlsx, sources=a.sources)
