"""案場 × 工作流盤點大表：從來源檔加上對照檔，產生 out/案場工作流盤點.xlsx。

用法：python3 -I build.py --sources <sources.json> --map-dir <對照檔資料夾> --out <輸出.xlsx>
三個參數都可以省略，省略時依序用環境變數 SWI_SOURCES／SWI_MAP_DIR／SWI_OUT，再不然用腳本自己資料夾
底下的 sources.json、對照檔/、out/案場工作流盤點.xlsx 當預設值。
只用 Python 標準函式庫，終端只印筆數。規則見卡的 DESIGN.md「每一格怎麼來」。
"""
import argparse
import csv
import json
import os
import pathlib
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

SCRIPT_DIR = pathlib.Path(__file__).resolve().parent
SOURCES_PATH = SCRIPT_DIR / "sources.json"
MAP = SCRIPT_DIR / "對照檔"
OUT = SCRIPT_DIR / "out" / "案場工作流盤點.xlsx"


def configure(sources=None, map_dir=None, out=None):
    """設定來源檔、對照檔資料夾、輸出路徑；順序是參數 > 環境變數 > 腳本自己資料夾底下的預設值。"""
    global SOURCES_PATH, MAP, OUT
    SOURCES_PATH = pathlib.Path(sources or os.environ.get("SWI_SOURCES") or SOURCES_PATH)
    MAP = pathlib.Path(map_dir or os.environ.get("SWI_MAP_DIR") or MAP)
    OUT = pathlib.Path(out or os.environ.get("SWI_OUT") or OUT)


def build_arg_parser():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--sources", help="sources.json 的路徑（預設：腳本資料夾/sources.json，或環境變數 SWI_SOURCES）")
    p.add_argument("--map-dir", help="對照檔資料夾（預設：腳本資料夾/對照檔，或環境變數 SWI_MAP_DIR）")
    p.add_argument("--out", help="輸出 xlsx 的路徑（預設：腳本資料夾/out/案場工作流盤點.xlsx，或環境變數 SWI_OUT）")
    return p

NS = {
    "m": "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
}
M = "{%s}" % NS["m"]

T2_HEADERS = [
    "專案編號", "案件簡稱", "電號", "在哪份清單", "行別（電費／租金）", "代號", "參數",
    "跟雲豹確認過沒有（確認過／推測／沒問到）", "確認的出處", "案場資料", "電號",
    "地主資料", "合約或租約全文", "租金參數", "電費單", "發票或收據",
    "EasyFlow 實際申請單", "系統能跑到哪一站（加上停住那一站的規格書編號）",
    "停在哪、下一步要跟誰要什麼",
]
PROV_HEADERS = ["表", "案場（專案編號／代號）", "行別", "欄名", "值", "出處"]


# ---------- xlsx 讀取（照 xlsx-headers.py 的讀法改） ----------
def _col_num(ref):
    n = 0
    for ch in re.match(r"[A-Z]+", ref).group():
        n = n * 26 + (ord(ch) - 64)
    return n


def _col_letters(n):
    s = ""
    while n > 0:
        n, rem = divmod(n - 1, 26)
        s = chr(65 + rem) + s
    return s


def _read_sheet_rows(z, ss, tgt):
    root = ET.fromstring(z.read(tgt))
    rows = []
    for r in root.iter(M + "row"):
        cells = {}
        for c in r.findall(M + "c"):
            v = c.find(M + "v")
            t = c.get("t")
            val = None
            if t == "s" and v is not None:
                val = ss[int(v.text)]
            elif t == "inlineStr":
                val = "".join(x.text or "" for x in c.iter(M + "t"))
            elif v is not None:
                val = v.text
            if val not in (None, ""):
                cells[_col_num(c.get("r"))] = val
        rows.append((int(r.get("r")), cells))
    return rows


def _sheet_rows(path, sheet_name, hdr_row):
    """回傳 (資料列[(列號,{欄:值})]，檔名)。只讀表頭之後有資料的列。"""
    z = zipfile.ZipFile(path)
    ss = []
    if "xl/sharedStrings.xml" in z.namelist():
        for si in ET.fromstring(z.read("xl/sharedStrings.xml")).findall("m:si", NS):
            ss.append("".join(t.text or "" for t in si.iter(M + "t")))
    wb = ET.fromstring(z.read("xl/workbook.xml"))
    rels = ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))
    rmap = {r.get("Id"): r.get("Target") for r in rels}
    tgt = None
    for sh in wb.find("m:sheets", NS):
        if sh.get("name") == sheet_name:
            tgt = rmap[sh.get("{%s}id" % NS["r"])]
    if tgt is None:
        raise KeyError(f"{path}: 找不到 sheet {sheet_name!r}")
    tgt = tgt.lstrip("/")
    if not tgt.startswith("xl/"):
        tgt = "xl/" + tgt
    rows = _read_sheet_rows(z, ss, tgt)
    body = [(rn, c) for rn, c in rows if rn > hdr_row and c]
    return body, pathlib.Path(path).name


def cite(prov, table, key, row_type, col_name, value, source):
    if value in (None, ""):
        return
    prov.append([table, key, row_type, col_name, str(value), source])


def norm_name(s):
    if not s:
        return ""
    s = str(s).strip()
    s = re.sub(r"\s+", "", s)
    return s


# 一、讀來源與合併案場（S5）
def load_sources():
    """讀 inventory/sources.json 與各份 xlsx（照 xlsx-headers.py 的寫法），每格記檔名、sheet、列、欄。"""
    cfg = json.loads(SOURCES_PATH.read_text(encoding="utf-8"))
    asset_body, asset_file = _sheet_rows(cfg["assetList"], "0. 資產及義務項目清單", 3)
    owner_body, _ = _sheet_rows(cfg["assetList"], "1. 土地所有權人清單", 2)
    station_body, station_file = _sheet_rows(cfg["stationList"], "工作表1", 1)
    bill_month_body, bill_file = _sheet_rows(cfg["billRecord"], "每月收件總紀錄", 1)
    bill_daily_body, _ = _sheet_rows(cfg["billRecord"], "每日收到的", 1)
    spec = json.loads(pathlib.Path(cfg["specExport"]).read_text(encoding="utf-8"))
    spec_items = {it["id"]: it for it in spec.get("items", []) if it.get("id")}
    return {
        "asset": {"rows": asset_body, "file": asset_file, "sheet": "0. 資產及義務項目清單"},
        "owner": {"rows": owner_body, "file": asset_file, "sheet": "1. 土地所有權人清單"},
        "station": {"rows": station_body, "file": station_file, "sheet": "工作表1"},
        "bill_month": {"rows": bill_month_body, "file": bill_file, "sheet": "每月收件總紀錄"},
        "bill_daily": {"rows": bill_daily_body, "file": bill_file, "sheet": "每日收到的"},
        "spec_items": spec_items,
        "spec_file": pathlib.Path(cfg["specExport"]).name,
    }


def _cite_sheet(info, rn, colno):
    return f"{info['file']}｜{info['sheet']}｜第{rn}列第{colno}欄"


def merge_sites(src, prov):
    """用電號把資產清單 122 列與發電案場清單合成 125 個案場；記錄每個案場在哪份清單。"""
    asset_info = src["asset"]
    station_info = src["station"]
    owner_rows = {rn: c for rn, c in src["owner"]["rows"]}

    sites = []
    meter_to_sites = {}
    for rn, c in asset_info["rows"]:
        pid = (c.get(3) or "").strip()
        meter = (c.get(18) or "").strip()
        if not pid or pid == "#N/A":
            # 對照檔/案場判斷.csv 的表頭說明：只在案場清單的用電號當鍵。
            pid = meter if meter and meter != "N/A" else f"專案編號缺(資產清單第{rn}列)"
        site = {
            "pid": pid,
            "short": c.get(4) or c.get(5) or "",
            "name": c.get(5) or "",
            "meters": [meter] if meter and meter != "N/A" else [],
            "lists": ["資產清單"],
            "meter_shared": None,
            "asset_row": rn,
            "asset_cells": c,
            "station_row": None,
            "station_cells": None,
            "owner_cells": None,
        }
        # 用 col3 專案編號串土地所有權人清單（同一個專案編號，取第一筆）
        for orn, oc in src["owner"]["rows"]:
            if (oc.get(3) or "").strip() == pid:
                site["owner_cells"] = oc
                site["owner_row"] = orn
                break
        sites.append(site)
        if meter and meter != "N/A":
            meter_to_sites.setdefault(meter, []).append(site)

    # 電號被 2 個以上專案共用（同一顆電表掛在多個專案上）：標記，不要 last-write-wins 蓋掉。
    for meter, shared_sites in meter_to_sites.items():
        if len(shared_sites) > 1:
            for s in shared_sites:
                others = len(shared_sites) - 1
                s["meter_shared"] = f"電號與另 {others} 個專案共用：" + "、".join(
                    x["pid"] for x in shared_sites if x is not s)

    used_station_rows = set()
    for rn, c in station_info["rows"]:
        meter = (c.get(10) or "").strip()
        for site in meter_to_sites.get(meter, []):
            site["lists"].append("發電案場清單")
            site["station_row"] = rn
            site["station_cells"] = c
            used_station_rows.add(rn)

    extra = 0
    for rn, c in station_info["rows"]:
        if rn in used_station_rows:
            continue
        meter = (c.get(10) or "").strip()
        # 對照檔/案場判斷.csv 的表頭說明：只在案場清單的用電號當鍵。
        pid = meter if meter else f"只在發電案場清單(電廠編號{c.get(1, rn)})"
        sites.append({
            "pid": pid,
            "short": c.get(6) or "",
            "name": c.get(6) or "",
            "meters": [meter] if meter else [],
            "lists": ["發電案場清單"],
            "asset_row": None,
            "asset_cells": None,
            "station_row": rn,
            "station_cells": c,
            "owner_cells": None,
        })
        extra += 1

    for site in sites:
        key = site["pid"]
        if site["asset_cells"]:
            for colno, name in ((3, "專案編號"), (4, "案件簡稱"), (5, "案件名稱"), (18, "台電電號")):
                cite(prov, "表二", key, "", name, site["asset_cells"].get(colno),
                     _cite_sheet(asset_info, site["asset_row"], colno))
        if site["station_cells"]:
            for colno, name in ((1, "電廠編號"), (6, "電廠名"), (10, "電號")):
                cite(prov, "表二", key, "", name, site["station_cells"].get(colno),
                     _cite_sheet(station_info, site["station_row"], colno))
    return sites


RE_SPLIT_CODE = re.compile(r"[＋+／/、]")


def _split_codes(s):
    return [t.strip() for t in RE_SPLIT_CODE.split(s or "") if t.strip()]


OWNER_GOV_KEYWORDS = ["政府", "公所", "國防部", "財政部", "教育部", "衛生局", "管理處", "國有財產", "鄉公所", "市公所", "區公所", "國中", "國小", "高中"]


def _guess_rp(owner_cells, owner_row, prov, key):
    if not owner_cells:
        return None, "沒問到", None
    name = (owner_cells.get(18) or "").strip()
    if not name:
        return None, "沒問到", None
    src_cite = f"資產管理清單2026.xlsx｜1. 土地所有權人清單｜第{owner_row}列第18欄"
    if any(k in name for k in OWNER_GOV_KEYWORDS):
        return "RP-3", "推測", src_cite
    if "公司" in name:
        return "RP-1", "推測", src_cite
    return "RP-4", "推測", src_cite


# 二、代號、覆寫、能跑到哪一站（S3）
def guess_codes(sites, src, prov):
    """照資產清單第 28、29、30、47、53 欄推 E／R 代號，照土地所有權人清單推 RP；值標「推測」。"""
    asset_info = src["asset"]
    rows = []
    for site in sites:
        key = site["pid"]
        meters_s = "；".join(site["meters"])
        lists_s = "、".join(dict.fromkeys(site["lists"]))
        base = {
            "專案編號": key, "案件簡稱": site["short"], "電號": meters_s,
            "在哪份清單": lists_s, "_site": site,
        }

        ac = site["asset_cells"]
        shared_note = [site["meter_shared"]] if site.get("meter_shared") else []
        # 電費行
        e_codes = []
        e_srcs = []
        e_params = list(shared_note)
        if ac:
            rn = site["asset_row"]
            invoice_judge = (ac.get(30) or "").strip()
            buyer = (ac.get(29) or "").strip()
            if invoice_judge == "V":
                e_codes.append("E-A")
                e_srcs.append(_cite_sheet(asset_info, rn, 30))
            if buyer and buyer not in ("無轉供", "N/A"):
                e_codes.extend(["E-B", "E-C"])
                e_srcs.append(_cite_sheet(asset_info, rn, 29))
                e_params.append(f"買方＝{buyer}")
        e_row = dict(base)
        e_row["行別（電費／租金）"] = "電費"
        if e_codes:
            e_row["代號"] = "＋".join(dict.fromkeys(e_codes))
            e_row["跟雲豹確認過沒有（確認過／推測／沒問到）"] = "推測"
            e_row["確認的出處"] = "；".join(dict.fromkeys(e_srcs))
        else:
            e_row["代號"] = "未判定"
            e_row["跟雲豹確認過沒有（確認過／推測／沒問到）"] = "沒問到"
            if ac:
                e_row["確認的出處"] = (
                    f"資產管理清單2026.xlsx｜0. 資產及義務項目清單｜第{site['asset_row']}列第28/29/30欄都沒命中規則"
                )
            else:
                e_row["確認的出處"] = "只在發電案場清單，資產清單沒有這個案場的資料"
        e_row["參數"] = "；".join(e_params) if e_params else "無"
        rows.append(e_row)

        # 租金行
        r_code = None
        r_src = None
        r_params = list(shared_note)
        if ac:
            rn = site["asset_row"]
            rent_judge = (ac.get(28) or "").strip()
            royalty = (ac.get(53) or "").strip()
            annual_rate = (ac.get(47) or "").strip()
            if rent_judge == "X":
                r_code = "R-0"
                r_src = _cite_sheet(asset_info, rn, 28)
            elif royalty and royalty != "N/A":
                r_code = "R-B"
                r_src = _cite_sheet(asset_info, rn, 53)
                r_params.append(f"權利金百分比＝{royalty}")
            elif annual_rate and annual_rate != "N/A":
                r_code = "R-A"
                r_src = _cite_sheet(asset_info, rn, 47)
                r_params.append(f"年租金率＝{annual_rate}")
        rp_code, rp_confirm, rp_src = (None, None, None)
        if r_code and r_code != "R-0":
            rp_code, rp_confirm, rp_src = _guess_rp(site["owner_cells"], site.get("owner_row"), prov, key)
        r_row = dict(base)
        r_row["行別（電費／租金）"] = "租金"
        if r_code == "R-0":
            r_row["代號"] = "R-0"
            r_row["跟雲豹確認過沒有（確認過／推測／沒問到）"] = "推測"
            r_row["確認的出處"] = r_src
        elif r_code:
            full_code = f"{r_code}／{rp_code}" if rp_code else f"{r_code}／RP未判定"
            r_row["代號"] = full_code
            r_row["跟雲豹確認過沒有（確認過／推測／沒問到）"] = "推測" if rp_code else "部分推測"
            srcs = [s for s in (r_src, rp_src) if s]
            if not rp_code:
                srcs.append("土地所有權人清單沒有姓名可判斷地主類型")
            r_row["確認的出處"] = "；".join(srcs)
        else:
            r_row["代號"] = "未判定"
            r_row["跟雲豹確認過沒有（確認過／推測／沒問到）"] = "沒問到"
            if ac:
                r_row["確認的出處"] = (
                    f"資產管理清單2026.xlsx｜0. 資產及義務項目清單｜第{site['asset_row']}列第28/47/53欄都沒命中規則"
                )
            else:
                r_row["確認的出處"] = "只在發電案場清單，資產清單沒有這個案場的資料"
        r_row["參數"] = "；".join(r_params) if r_params else "無"
        rows.append(r_row)

    return rows


def apply_overrides(rows, prov):
    """讀 對照檔/案場判斷.csv，覆寫代號、參數、確認狀態、停在哪；出處改成「對照檔/案場判斷.csv:<行>」。"""
    path = MAP / "案場判斷.csv"
    with path.open(encoding="utf-8-sig", newline="") as f:
        csv_rows = list(csv.DictReader(f))
    overrides = {}
    for i, row in enumerate(csv_rows, start=2):
        pid = (row.get("專案編號（只在案場清單的用電號）") or "").strip()
        row_type = (row.get("行別（電費／租金）") or "").strip()
        if not pid or not row_type:
            continue
        overrides[(pid, row_type)] = (row, i)

    n = 0
    for row in rows:
        key = (row["專案編號"], row["行別（電費／租金）"])
        if key in overrides:
            ov, rownum = overrides[key]
            src_cite = f"對照檔/案場判斷.csv:{rownum}"
            if (ov.get("代號") or "").strip():
                row["代號"] = ov["代號"].strip()
            if (ov.get("參數") or "").strip():
                row["參數"] = ov["參數"].strip()
            if (ov.get("確認狀態（確認過／推測／沒問到）") or "").strip():
                row["跟雲豹確認過沒有（確認過／推測／沒問到）"] = ov["確認狀態（確認過／推測／沒問到）"].strip()
            row["確認的出處"] = src_cite
            if (ov.get("停在哪、下一步") or "").strip():
                row["_override_next"] = ov["停在哪、下一步"].strip()
            for colname in ("代號", "參數", "確認狀態（確認過／推測／沒問到）"):
                val = ov.get(colname)
                if (val or "").strip():
                    cite(prov, "表二", row["專案編號"], row["行別（電費／租金）"], colname, val, src_cite)
            n += 1
    return rows, n


def load_code_table():
    path = MAP / "代號表.csv"
    with path.open(encoding="utf-8-sig", newline="") as f:
        csv_rows = list(csv.DictReader(f))
    table = {}
    chain_col = "站（依序；站名=規格編號）"
    status_col = "站狀態補充"
    for i, row in enumerate(csv_rows, start=2):
        code = (row.get("代號") or "").strip()
        if not code:
            continue
        stations = []
        chain = row.get(chain_col) or ""
        if "=" in chain:
            for seg in chain.split("→"):
                seg = seg.strip()
                if "=" not in seg:
                    continue
                name, codes_s = seg.split("=", 1)
                stations.append((name.strip(), [x.strip() for x in codes_s.split(";") if x.strip()]))
        overrides = {}
        status_s = (row.get(status_col) or "").strip()
        if status_s:
            for seg in status_s.split("；"):
                seg = seg.strip()
                if not seg or "=" not in seg:
                    continue
                name, rest = seg.split("=", 1)
                m = re.match(r"\s*(done|partial|todo)", rest)
                if m:
                    overrides[name.strip()] = m.group(1)
        table[code] = {"stations": stations, "overrides": overrides, "row": i, "raw": row}
    return table, csv_rows


def _station_status(name, codes, spec_items, overrides):
    if name in overrides:
        return overrides[name]
    if not codes:
        return "todo"
    order = {"done": 2, "partial": 1, "todo": 0}
    worst = "done"
    for c in codes:
        it = spec_items.get(c)
        st = "todo"
        if it and it.get("fields"):
            st = it["fields"].get("impl") or "todo"
        if order.get(st, 0) < order.get(worst, 0):
            worst = st
    return worst


def _reach_for_code(code, table, spec_items):
    entry = table.get(code)
    if not entry or not entry["stations"]:
        return None
    reached = None
    stuck = None
    reached_idx = -1
    stuck_idx = None
    for idx, (name, codes) in enumerate(entry["stations"]):
        status = _station_status(name, codes, spec_items, entry["overrides"])
        if status in ("partial", "done"):
            reached = name
            reached_idx = idx
        else:
            stuck = name
            stuck_idx = idx
            break
    return {"reached": reached, "stuck": stuck, "reached_idx": reached_idx, "stuck_idx": stuck_idx,
            "stations": entry["stations"]}


def reach(rows, code_table, spec_items):
    """代號表的站×規格匯出 impl：連續 partial／done 的最後一站是能跑到，第一個 todo 是停住；資料缺了往下壓。"""
    for row in rows:
        if row["代號"] in ("", "未判定"):
            row["系統能跑到哪一站（加上停住那一站的規格書編號）"] = "未判定（沒有代號可查，見「確認的出處」）"
            row["停在哪、下一步要跟誰要什麼"] = "沒問到代號，先跟雲豹確認工作流"
            continue
        codes = _split_codes(row["代號"])

        bill_missing = (row.get("電費單") or "").startswith("缺")
        rent_missing = (row.get("租金參數") or "").startswith("缺")

        parts = []
        stuck_texts = []
        for code in codes:
            if code == "RP未判定":
                parts.append("RP未判定:沒問到地主類型")
                stuck_texts.append("跟雲豹要：地主類型（公司／機關／個人）才能判斷怎麼付")
                continue
            r = _reach_for_code(code, code_table, spec_items)
            if r is None:
                parts.append(f"{code}:不跑（這個代號不跑這條工作流，或代號表沒有這個代號）")
                continue
            reached, stuck = r["reached"], r["stuck"]
            stations = r["stations"]
            if rent_missing and row["行別（電費／租金）"] == "租金" and stations:
                reached, stuck = None, stations[0][0]
            elif bill_missing and stations:
                first = stations[0][0]
                if "收件" in first or r["stuck_idx"] in (None, 0) or r["reached_idx"] < 0:
                    reached, stuck = None, first
            if stuck:
                parts.append(f"{code}:停在{stuck}")
                stuck_texts.append(f"系統：{code} 的「{stuck}」還沒做")
            else:
                parts.append(f"{code}:能跑到{reached or '(無站)'}" )
        row["系統能跑到哪一站（加上停住那一站的規格書編號）"] = "；".join(parts)
        if stuck_texts:
            row["停在哪、下一步要跟誰要什麼"] = "；".join(stuck_texts)
        elif bill_missing:
            row["停在哪、下一步要跟誰要什麼"] = "跟雲豹要：電費單"
        elif rent_missing:
            row["停在哪、下一步要跟誰要什麼"] = "跟雲豹要：租金參數"
        else:
            row["停在哪、下一步要跟誰要什麼"] = "目前都通過，持續追蹤"
        if "_override_next" in row:
            row["停在哪、下一步要跟誰要什麼"] = row["_override_next"]
    return rows


# 三、資料到齊 8 欄（S4）
def _load_map_csv(name):
    path = MAP / name
    with path.open(encoding="utf-8-sig", newline="") as f:
        return list(csv.DictReader(f))


def _build_name_index(sites):
    idx = {}
    for site in sites:
        for nm in (site["short"], site["name"], site["station_cells"].get(6) if site["station_cells"] else None):
            n = norm_name(nm)
            if n:
                idx.setdefault(n, set()).add(site["pid"])
    return idx


def _resolve_name(raw, idx, override_map, unmatched):
    n = norm_name(raw)
    if not n:
        return None
    if n in override_map:
        return override_map[n]
    pids = idx.get(n)
    if pids and len(pids) == 1:
        return next(iter(pids))
    for key, pids2 in idx.items():
        if key and len(pids2) == 1 and (key in n or n in key):
            return next(iter(pids2))
    unmatched.add(raw)
    return None


def completeness(rows, src, prov, sites):
    """第 10～17 欄：有／缺／部分／不適用，加筆數；來源見 DESIGN.md。"""
    name_map_rows = _load_map_csv("名稱對照.csv")
    name_override = {}
    for i, r in enumerate(name_map_rows, start=2):
        bn = norm_name(r.get("電費單紀錄裡的案場名稱"))
        pid = (r.get("專案編號") or "").strip()
        if bn and pid:
            name_override[bn] = pid

    file_map_rows = _load_map_csv("檔案對照.csv")
    files_by_pid = {}
    for i, r in enumerate(file_map_rows, start=2):
        pid = (r.get("專案編號") or "").strip()
        typ = (r.get("類型（租約／付款申請單／EF 截圖／EF 申請單／電費單 PDF／發票或收據）") or "").strip()
        rtype = (r.get("行別（電費／租金）") or "").strip()
        if pid:
            files_by_pid.setdefault(pid, []).append((typ, rtype))

    name_idx = _build_name_index(sites)
    unmatched = set()

    bill_info = src["bill_month"]
    bill_by_pid = {}
    for rn, c in bill_info["rows"]:
        nm = c.get(2)
        pid = _resolve_name(nm, name_idx, name_override, unmatched)
        if not pid:
            continue
        months = sum(1 for k, v in c.items() if k >= 3 and v not in (None, ""))
        bill_by_pid[pid] = bill_by_pid.get(pid, 0) + months

    daily_info = src["bill_daily"]
    daily_by_pid = {}
    for rn, c in daily_info["rows"]:
        nm = c.get(5)
        pid = _resolve_name(nm, name_idx, name_override, unmatched)
        if not pid:
            continue
        has_invoice = 1 if (c.get(10) or c.get(16)) else 0
        if has_invoice:
            daily_by_pid[pid] = daily_by_pid.get(pid, 0) + 1

    site_by_pid = {s["pid"]: s for s in sites}

    # 名稱沒對上、但長得像這個案場的，記下來：電費單/發票或收據欄要跟「真的缺」分開顯示。
    un_norm = {norm_name(nm) for nm in unmatched if norm_name(nm)}
    possible_mismatch = {}
    for nm in unmatched:
        n = norm_name(nm)
        if not n:
            continue
        for site in sites:
            cands = [site["short"], site["name"]]
            if site["station_cells"]:
                cands.append(site["station_cells"].get(6))
            for cand in cands:
                cn = norm_name(cand)
                if cn and len(cn) >= 2 and (cn in n or n in cn):
                    possible_mismatch[site["pid"]] = possible_mismatch.get(site["pid"], 0) + 1
    for site in sites:
        if any(site["pid"] in name_idx.get(n, ()) for n in un_norm):
            possible_mismatch[site["pid"]] = possible_mismatch.get(site["pid"], 0) + 1

    for row in rows:
        pid = row["專案編號"]
        row_type = row["行別（電費／租金）"]
        site = site_by_pid.get(pid)
        ac = site["asset_cells"] if site else None
        sc = site["station_cells"] if site else None
        oc = site["owner_cells"] if site else None

        # 10 案場資料
        n = sum(1 for v in (
            (ac.get(4) if ac else None), (ac.get(5) if ac else None),
            (ac.get(6) if ac else None), (ac.get(7) if ac else None),
        ) if v)
        row["案場資料"] = f"有({n})" if n >= 3 else (f"部分({n})" if n else "缺(0)")

        # 11 電號
        nm = len(site["meters"]) if site else 0
        row["電號"] = f"有({nm})" if nm else "缺(0)"

        # 12 地主資料／14 租金參數（電費行不適用）
        if row_type == "電費":
            row["地主資料"] = "不適用"
            row["租金參數"] = "不適用"
        else:
            cnt = sum(1 for v in ((oc.get(18) if oc else None), (oc.get(21) if oc else None)) if v)
            row["地主資料"] = f"有({cnt})" if cnt == 2 else (f"部分({cnt})" if cnt else "缺(0)")
            code0 = _split_codes(row["代號"])[0] if row["代號"] else ""
            pcnt = 0
            if ac:
                if code0 == "R-A":
                    pcnt = sum(1 for v in (ac.get(47), ac.get(49)) if v and v != "N/A")
                elif code0 == "R-B":
                    pcnt = sum(1 for v in (ac.get(53), ac.get(55)) if v and v != "N/A")
            row["租金參數"] = f"有({pcnt})" if pcnt >= 2 else (f"部分({pcnt})" if pcnt else "缺(0)")

        mismatch_n = possible_mismatch.get(pid, 0)

        # 15 電費單：兩邊都有才算「有」（不加字首），只有一邊有算「部分」（橘），兩邊都沒有算「缺」（紅）。
        months = bill_by_pid.get(pid, 0)
        pdfs = sum(1 for typ, rtype in files_by_pid.get(pid, []) if typ == "電費單PDF")
        if months and pdfs:
            row["電費單"] = f"紀錄{months}月／PDF{pdfs}份"
        elif months or pdfs:
            row["電費單"] = f"部分(紀錄{months}月／PDF{pdfs}份)"
        elif mismatch_n:
            row["電費單"] = f"未對上（名稱對照.csv 沒有對應，{mismatch_n} 個類似名稱沒對到）"
        else:
            row["電費單"] = "缺(紀錄0月／PDF0份)"

        # 16 發票或收據
        if row_type == "電費":
            n16 = daily_by_pid.get(pid, 0)
        else:
            n16 = sum(1 for typ, rtype in files_by_pid.get(pid, []) if typ == "發票或收據")
        if n16:
            row["發票或收據"] = f"有({n16})"
        elif row_type == "電費" and mismatch_n:
            row["發票或收據"] = f"未對上（名稱對照.csv 沒有對應，{mismatch_n} 個類似名稱沒對到）"
        else:
            row["發票或收據"] = "缺(0)"

        # 17 EasyFlow 實際申請單
        n17 = sum(1 for typ, rtype in files_by_pid.get(pid, []) if typ in ("EF 截圖", "EF 申請單"))
        row["EasyFlow 實際申請單"] = f"有({n17})" if n17 else "缺(0)"

        # 13 合約或租約全文
        n13 = sum(1 for typ, rtype in files_by_pid.get(pid, []) if typ == "租約")
        row["合約或租約全文"] = f"有({n13})" if n13 else "缺(0)"

    return len(unmatched)


# 四、寫 xlsx 三張表（S1）
def _esc(s):
    return (str(s).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace('"', "&quot;"))


# S6：表二上色。第 10～17 欄是「資料到齊」欄，缺／未對上→紅，部分／含推測→橘；
# 其餘（含其他 9 欄）照「每個案場兩行算一組」的單雙組底色（單數白、雙數淡藍）。紅橘蓋在組底色上面。
T2_COLOR_COLS = set(range(10, 18))
STYLE_ODD = 0       # 單數組：白底（沿用無 fill 的預設 xf）
STYLE_HEADER = 1
STYLE_EVEN = 2       # 雙數組：淡藍 DDEBF7
STYLE_RED = 3        # 缺／未對上：FFC7CE
STYLE_ORANGE = 4     # 部分／含推測：FFE0B2


def t2_cell_style(row_idx0, col_idx1, value):
    """row_idx0：這一列在表二資料列裡第幾列（0 起算）；col_idx1：第幾欄（1 起算）。"""
    group_idx0 = row_idx0 // 2
    group_even = (group_idx0 % 2) == 1
    base = STYLE_EVEN if group_even else STYLE_ODD
    if col_idx1 in T2_COLOR_COLS:
        v = value or ""
        if v.startswith("缺") or v.startswith("未對上"):
            return STYLE_RED
        if v.startswith("部分") or "推測" in v:
            return STYLE_ORANGE
    return base


def _sheet_xml(headers, data_rows, style_fn=None):
    ncols = len(headers)
    last_col = _col_letters(ncols)
    nrows = len(data_rows) + 1
    out = [
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
        '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">',
        '<sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" '
        'activePane="bottomLeft" state="frozen"/></sheetView></sheetViews>',
        f'<dimension ref="A1:{last_col}{nrows}"/>',
        '<sheetData>',
    ]
    hdr_cells = []
    for i, h in enumerate(headers, start=1):
        ref = f"{_col_letters(i)}1"
        hdr_cells.append(f'<c r="{ref}" t="inlineStr" s="1"><is><t xml:space="preserve">{_esc(h)}</t></is></c>')
    out.append(f'<row r="1">{"".join(hdr_cells)}</row>')
    for ridx, row in enumerate(data_rows, start=2):
        row_idx0 = ridx - 2
        cells = []
        for i, v in enumerate(row, start=1):
            ref = f"{_col_letters(i)}{ridx}"
            if style_fn is not None:
                sid = style_fn(row_idx0, i, v)
                if v in (None, ""):
                    cells.append(f'<c r="{ref}" s="{sid}"/>')
                else:
                    cells.append(
                        f'<c r="{ref}" t="inlineStr" s="{sid}"><is><t xml:space="preserve">{_esc(v)}</t></is></c>')
            else:
                if v in (None, ""):
                    continue
                cells.append(f'<c r="{ref}" t="inlineStr"><is><t xml:space="preserve">{_esc(v)}</t></is></c>')
        out.append(f'<row r="{ridx}">{"".join(cells)}</row>')
    out.append('</sheetData>')
    out.append(f'<autoFilter ref="A1:{last_col}{nrows}"/>')
    out.append('</worksheet>')
    return "".join(out)


def write_xlsx(table1, table2, sources_rows):
    """zipfile 寫三張表（sharedStrings 或 inlineStr、粗體表頭、凍結首列、autoFilter）：寫到 out/案場工作流盤點.xlsx；終端只印筆數。"""
    OUT.parent.mkdir(parents=True, exist_ok=True)
    sheets = [("表一 代號", table1), ("表二 案場×工作流", table2), ("出處", sources_rows)]

    content_types = [
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">',
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>',
        '<Default Extension="xml" ContentType="application/xml"/>',
        '<Override PartName="/xl/workbook.xml" '
        'ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>',
        '<Override PartName="/xl/styles.xml" '
        'ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>',
    ]
    for i in range(1, len(sheets) + 1):
        content_types.append(
            f'<Override PartName="/xl/worksheets/sheet{i}.xml" '
            'ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>')
    content_types.append('</Types>')

    root_rels = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" '
        'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" '
        'Target="xl/workbook.xml"/></Relationships>')

    wb_sheets_xml = "".join(
        f'<sheet name="{_esc(name)}" sheetId="{i}" r:id="rId{i}"/>'
        for i, (name, _) in enumerate(sheets, start=1))
    workbook_xml = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        f'<sheets>{wb_sheets_xml}</sheets></workbook>')

    wb_rels_items = [
        f'<Relationship Id="rId{i}" '
        'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" '
        f'Target="worksheets/sheet{i}.xml"/>'
        for i in range(1, len(sheets) + 1)
    ]
    wb_rels_items.append(
        f'<Relationship Id="rId{len(sheets) + 1}" '
        'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" '
        'Target="styles.xml"/>')
    workbook_rels = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        + "".join(wb_rels_items) + '</Relationships>')

    styles_xml = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
        '<fonts count="2"><font><sz val="11"/><name val="Calibri"/></font>'
        '<font><b/><sz val="11"/><name val="Calibri"/></font></fonts>'
        '<fills count="5">'
        '<fill><patternFill patternType="none"/></fill>'
        '<fill><patternFill patternType="gray125"/></fill>'
        '<fill><patternFill patternType="solid"><fgColor rgb="FFDDEBF7"/><bgColor indexed="64"/></patternFill></fill>'
        '<fill><patternFill patternType="solid"><fgColor rgb="FFFFC7CE"/><bgColor indexed="64"/></patternFill></fill>'
        '<fill><patternFill patternType="solid"><fgColor rgb="FFFFE0B2"/><bgColor indexed="64"/></patternFill></fill>'
        '</fills>'
        '<borders count="1"><border/></borders>'
        '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0"/></cellStyleXfs>'
        '<cellXfs count="5">'
        '<xf numFmtId="0" fontId="0" fillId="0" applyFont="1"/>'
        '<xf numFmtId="0" fontId="1" fillId="0" applyFont="1"/>'
        '<xf numFmtId="0" fontId="0" fillId="2" applyFont="1" applyFill="1"/>'
        '<xf numFmtId="0" fontId="0" fillId="3" applyFont="1" applyFill="1"/>'
        '<xf numFmtId="0" fontId="0" fillId="4" applyFont="1" applyFill="1"/>'
        '</cellXfs>'
        '</styleSheet>')

    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", "".join(content_types))
        z.writestr("_rels/.rels", root_rels)
        z.writestr("xl/workbook.xml", workbook_xml)
        z.writestr("xl/_rels/workbook.xml.rels", workbook_rels)
        z.writestr("xl/styles.xml", styles_xml)
        for i, (name, (headers, data_rows)) in enumerate(sheets, start=1):
            sf = t2_cell_style if name == "表二 案場×工作流" else None
            z.writestr(f"xl/worksheets/sheet{i}.xml", _sheet_xml(headers, data_rows, sf))


def main():
    args = build_arg_parser().parse_args()
    configure(sources=args.sources, map_dir=args.map_dir, out=args.out)

    src = load_sources()
    code_table, code_csv_rows = load_code_table()
    prov = []

    # 表一：直接用代號表.csv，每個有值的格子記一列出處
    t1_headers = list(code_csv_rows[0].keys()) if code_csv_rows else []
    t1_rows = []
    for i, r in enumerate(code_csv_rows, start=2):
        t1_rows.append([r.get(h, "") for h in t1_headers])
        for h in t1_headers:
            cite(prov, "表一", r.get("代號", ""), "", h, r.get(h), f"對照檔/代號表.csv:{i}")

    sites = merge_sites(src, prov)
    rows = guess_codes(sites, src, prov)
    rows, override_n = apply_overrides(rows, prov)
    unmatched_n = completeness(rows, src, prov, sites)
    reach(rows, code_table, src["spec_items"])

    for row in rows:
        key = row["專案編號"]
        rtype = row["行別（電費／租金）"]
        for h in T2_HEADERS:
            v = row.get(h)
            if v not in (None, ""):
                if h in ("電費單", "發票或收據") and str(v).startswith("未對上"):
                    src_text = "名稱對不上：billRecord 的案場名稱跟資產清單對不起來，見 README「已知限制」"
                else:
                    src_text = row.get("確認的出處") or "推算（見 DESIGN.md「每一格怎麼來」）"
                cite(prov, "表二", key, rtype, h, v, src_text)

    t2_rows = [[row.get(h, "") for h in T2_HEADERS] for row in rows]

    write_xlsx((t1_headers, t1_rows), (T2_HEADERS, t2_rows), (PROV_HEADERS, prov))

    print(f"案場數 {len(sites)}")
    print(f"表二 {len(t2_rows)} 行")
    print(f"出處 {len(prov)} 列")
    print(f"名稱對不到 {unmatched_n} 個")
    print(f"案場判斷.csv 覆寫 {override_n} 行")
    print(f"輸出：{OUT}")


if __name__ == "__main__":
    main()
