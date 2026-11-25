"""Prepare reusable rules from a supplied asset workbook. Never infer confirmed status.

Inputs and output contain customer data and must remain outside Git.
Overrides are keyed by normalized meter AND project code; a human-reviewed override
is required for confirmed rules. No legal effective date is inferred from signing date.
"""
import argparse, hashlib, json, re
from pathlib import Path
from datetime import date, datetime

def meter(v):
    s=re.sub(r'[^0-9]', '',str(v or ''))
    return s if len(s)==11 else None

def scalar(v):
    return v.isoformat()[:10] if isinstance(v,(date,datetime)) else v

def prepare(source, stations, overrides):
    import openpyxl
    digest=hashlib.sha256(source.read_bytes()).hexdigest()
    ws=openpyxl.load_workbook(source,data_only=True)['0. 資產及義務項目清單']
    prepared=[];unmatched=[]; seen=set()
    for r in range(4,ws.max_row+1):
        c=lambda col:scalar(ws[f'{col}{r}'].value)
        project=str(c('C') or '').strip(); key=meter(c('R'))
        if not project or not key: continue
        hits=[s for s in stations if meter(s.get('customerNumber'))==key or key in s.get('meterKeys',[])]
        # A shared meter is insufficient to uniquely select one project/rule.
        if len(hits)!=1 or sum(1 for rr in range(4,ws.max_row+1) if meter(ws[f'R{rr}'].value)==key)!=1:
            unmatched.append({'projectCode':project,'meterKey':key,'row':r,'reason':'電號未唯一對應；不可猜測案場'});continue
        source_key=f'asset-rent:{project}'
        if source_key in seen: raise ValueError('duplicate project '+project)
        seen.add(source_key)
        raw={col:c(col) for col in ['C','D','R','W','Y','AA','AB','AC','AF','AS','AT','AU','AV','AW','AY','AZ','BA','BB','BC','BD']}
        rule={'status':'pending','calculationType':'percentage' if isinstance(c('BA'),(int,float)) else 'pendingUnknown','payee':c('W'),
              'basis':{'V':'gross','X':'untaxed'}.get(c('AY')),
              'percent':str(round(float(c('BA'))*100,8)) if isinstance(c('BA'),(int,float)) else None,
              'cycleMonths':{'一個月':1,'三個月':3,'六個月':6,'一年':12}.get(c('BC')),
              'validFrom':None,'validTo':None,
              'ruleSource':f'{source.name}／{ws.title} 第{r}列',
              'sourceKey':source_key,'sourceSha256':digest,'sourceRow':r,'sourceProjectCode':project,
              'sourceValues':raw,'leaseEvidence':{'status':'missing','attachmentIds':[]},
              'notes':['資產總表來源已建檔；未確認參數保留待確認，不自動啟用。','簽署日期不是計算規則生效日期；原值保留於來源。']}
        override=overrides.get(project+'|'+key)
        if override: rule.update(override)
        prepared.append({'stationId':hits[0]['id'],'stationName':hits[0]['name'],'customerNumber':c('R'),'rule':rule})
    return {'sourceFile':source.name,'sourceSha256':digest,'items':prepared,'unmatched':unmatched}

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--asset',required=True,type=Path);p.add_argument('--stations',required=True,type=Path);p.add_argument('--overrides',required=True,type=Path);p.add_argument('--out',required=True,type=Path)
    a=p.parse_args();result=prepare(a.asset,json.loads(a.stations.read_text(encoding='utf-8-sig')),json.loads(a.overrides.read_text(encoding='utf-8-sig')))
    a.out.parent.mkdir(parents=True,exist_ok=True);a.out.write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf8')
    print(json.dumps({'prepared':len(result['items']),'confirmed':sum(i['rule']['status']=='confirmed' for i in result['items']),'unmatched':len(result['unmatched'])}))
