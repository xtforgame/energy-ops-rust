"""Read customer workbooks without modifying them; emit portable JSON/CSV and SQL staging rows."""
import argparse, csv, hashlib, json, re
from pathlib import Path
from collections import defaultdict
from datetime import date, datetime

def normalize_meter(value):
    value=str(value or '').strip()
    if not re.fullmatch(r'[0-9\s-]+',value):return None
    value=re.sub(r'\D','',value)
    return value if len(value)==11 else None

def consolidate(rows):
    groups=defaultdict(list)
    for row in rows:groups[row['meterKey']].append(row)
    result=[]; conflicts=[]
    for key, group in groups.items():
        if len({(r['plantNo'],r['name']) for r in group})>1:
            conflicts.append({'meterKey':key,'reason':'同電號對應不同案場，待確認','rows':group})
        else:result.append(group[0])
    return result,conflicts

def scalar(v):
    if isinstance(v,(datetime,date)):return v.isoformat()
    if v is None:return None
    return str(v).strip()

def dump(path,value):path.write_text(json.dumps(value,ensure_ascii=False,indent=2),encoding='utf8')

def write_csv(path,rows):
    keys=list(dict.fromkeys(k for r in rows for k in r))
    with path.open('w',encoding='utf-8-sig',newline='') as f:
        w=csv.DictWriter(f,fieldnames=keys);w.writeheader()
        for r in rows:w.writerow({k:json.dumps(v,ensure_ascii=False) if isinstance(v,(dict,list)) else v for k,v in r.items()})

def prepare(root,out,verified_examples=None):
    import openpyxl
    out.mkdir(parents=True,exist_ok=True)
    manifest=[];staging=[];pending=[];schemas=[]
    def locate(pattern):
        matches=[p for p in root.rglob(pattern) if not p.name.startswith('~$')]
        if len(matches)!=1:raise ValueError(f'Expected exactly one source for {pattern}; got {len(matches)}')
        p=matches[0];sha=hashlib.sha256(p.read_bytes()).hexdigest()
        manifest.append({'filename':p.name,'sha256':sha,'size':p.stat().st_size})
        return p,sha
    def read(pattern,sheet,header):
        p,sha=locate(pattern);w=openpyxl.load_workbook(p,data_only=True,read_only=True);s=w[sheet]
        records=[]
        for i,row in enumerate(s.iter_rows(values_only=True),1):
            if i<=header:
                schemas.append({'sourceFile':p.name,'sheet':sheet,'row':i,'cells':{openpyxl.utils.get_column_letter(j):scalar(v) for j,v in enumerate(row,1) if v is not None}})
                continue
            cells={openpyxl.utils.get_column_letter(j):scalar(v) for j,v in enumerate(row,1) if v is not None}
            if not cells:continue
            key=hashlib.sha256(f'{sha}|{sheet}|{i}'.encode()).hexdigest()
            r={'sourceKey':key,'sourceFile':p.name,'sourceSha256':sha,'sheet':sheet,'sourceRow':i,'cells':cells}
            records.append(r)
        w.close();return records
    masters=read('*超強*.xlsx','自動化所需欄位',1)
    candidates=[]
    for r in masters:
        c=r['cells'];key=normalize_meter(c.get('H'))
        if not any(c.get(k) for k in ['A','D','H']):continue
        staging.append(dict(r,kind='station_source'))
        if not key or not c.get('D') or not c.get('E'):
            pending.append({'sourceFile':r['sourceFile'],'row':r['sourceRow'],'reason':'電號格式／案場名稱／公司不完整','cells':c});continue
        candidates.append({'meterKey':key,'customerNumber':c['H'],'plantNo':c.get('A'),'name':c['D'],'spv':c['E'],
            'spvShort':c.get('F'),'capacityKw':c.get('G'),'region':c.get('B'),
            'invoice':{'counterpartyTaxId':c.get('C'),'contact':c.get('I'),'delivery':c.get('J'),'email1':c.get('K'),'email2':c.get('L'),'address':c.get('M')},
            'sourceFile':r['sourceFile'],'sourceRow':r['sourceRow'],'sourceKey':r['sourceKey']})
    stations,conflicts=consolidate(candidates);pending.extend(conflicts)
    bymeter={s['meterKey']:s for s in stations}
    assets=read('資產管理清單2026.xlsx','0. 資產及義務項目清單',3)
    rules=[]
    for r in assets:
        c=r['cells']
        if not c.get('E'):continue
        staging.append(dict(r,kind='asset_source'))
        key=normalize_meter(c.get('R'))
        rule={'meterKey':key,'assetName':c.get('E'),'alias':c.get('D'),'projectCode':c.get('C'),
            'lessor':c.get('W'),'basisRaw':c.get('AY'),'taxMethodRaw':c.get('AZ'),'ratioRaw':c.get('BA'),
            'frequencyRaw':c.get('BC'),'paymentTimingRaw':c.get('BD'),'landAreaRaw':c.get('AS'),'landValueRaw':c.get('AT'),
            'annualRateRaw':c.get('AU'),'fixedAmountRaw':c.get('AW'),'contractSignedRaw':c.get('Y'),'contractEndRaw':c.get('AA'),
            'sourceFile':r['sourceFile'],'sheet':r['sheet'],'sourceRow':r['sourceRow'],'sourceKey':r['sourceKey'],
            'status':'來源已提供；自動計算參數待確認','automaticPaymentAllowed':False}
        if key in bymeter:
            bymeter[key].setdefault('assetSources',[]).append({'sourceKey':r['sourceKey'],'sourceRow':r['sourceRow'],'name':c.get('E'),'projectCode':c.get('C')})
        else:pending.append({'sourceFile':r['sourceFile'],'row':r['sourceRow'],'meterKey':key,'reason':'資產電號未唯一對上總表；不以近似名稱合併'})
        rules.append(rule)
    histories=read('資產管理清單2026.xlsx','2-1.租金付款紀錄',2)
    for r in histories:staging.append(dict(r,kind='rent_payment_history_source'))
    electricity=read('【電費單紀錄】.xlsx','每日收到的',1)
    for r in electricity:staging.append(dict(r,kind='electricity_history_source'))
    # Source formula/cached-value evidence: no cached number is promoted into a new rule silently.
    for pattern in ['11507-11510_蘇澳四號倉庫租金試算.xlsx','E金聯麻豆租金計算*.xlsx']:
        p,sha=locate(pattern);w=openpyxl.load_workbook(p,data_only=False,read_only=True);cached=openpyxl.load_workbook(p,data_only=True,read_only=True)
        for s in w:
            for number,row in enumerate(s,1):
                cells={c.coordinate:{'value':scalar(c.value),'cached':scalar(cached[s.title][c.coordinate].value)} for c in row if c.value is not None}
                if not cells:continue
                key=hashlib.sha256(f'{sha}|{s.title}|{number}'.encode()).hexdigest()
                staging.append({'kind':'rent_calculation_source','sourceKey':key,'sourceFile':p.name,'sourceSha256':sha,'sheet':s.title,'sourceRow':number,'cells':cells})
        w.close();cached.close()
    # No local attachment IDs: these vary by deployment and must be bound by file hash after upload.
    for example in verified_examples or []:
        key=example['meterKey'];filename=example['pdfFile'];period=[example['periodStart'],example['periodEnd']]
        p,sha=locate(filename)
        matches=[r for r in rules if r['meterKey']==key]
        if len(matches)!=1:raise ValueError('Expected unique verified rental source: '+key)
        matches[0]['verifiedExample']={**example,'pdfSha256':sha,'method':'先前文件驗算；本輪未實跑平台'}
        matches[0]['status']='部分條款及歷史範例已核對；未完成全期間驗收'
        matches[0]['pending']=example['pending']
    # One source row can only be staged once, even if files are reused by multiple sheets.
    assert len({r['sourceKey'] for r in staging})==len(staging)
    manifest=list({r['sha256']:r for r in manifest}.values())
    for name,rows in [('stations',stations),('rent-rules',rules),('source-rows',staging),('pending',pending)]:
        dump(out/(name+'.json'),rows)
        if name!='source-rows':write_csv(out/(name+'.csv'),rows)
    write_csv(out/'rent-payment-history.csv',[{**{k:v for k,v in r.items() if k!='cells'},**r['cells']} for r in staging if r['kind']=='rent_payment_history_source'])
    write_csv(out/'electricity-history.csv',[{**{k:v for k,v in r.items() if k!='cells'},**r['cells']} for r in staging if r['kind']=='electricity_history_source'])
    dump(out/'source-schemas.json',schemas)
    dump(out/'manifest.json',{'version':1,'sources':manifest,'counts':{'stations':len(stations),'rentRules':len(rules),'sourceRows':len(staging),'pending':len(pending)},'databaseApplied':False,'platformRun':False})
    # psql literal data section; only used by the generic transactional importer.
    payload=json.dumps({'stations':stations,'rules':rules,'rows':staging},ensure_ascii=False,separators=(',',':'))
    tag='$energy_import_'+hashlib.sha256(payload.encode()).hexdigest()[:16]+'$'
    (out/'data.sql').write_text('INSERT INTO import_payload VALUES ('+tag+payload+tag+'::jsonb);\n',encoding='utf8')
    print(json.dumps({'output':str(out),'stations':len(stations),'rentRules':len(rules),'pending':len(pending),'sourceRows':len(staging)},ensure_ascii=False))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--sources',required=True,type=Path);p.add_argument('--out',required=True,type=Path);p.add_argument('--verified-examples',type=Path)
    a=p.parse_args();prepare(a.sources,a.out,json.loads(a.verified_examples.read_text(encoding='utf8')) if a.verified_examples else None)
