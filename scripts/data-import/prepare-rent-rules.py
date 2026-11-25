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

def number(v):
    return v if isinstance(v,(int,float)) and not isinstance(v,bool) else None

def annual_amount(v):
    m=re.fullmatch(r'\s*([\d,]+(?:\.\d+)?)\s*(?:/年|\(年\))\s*',str(v or ''))
    return float(m[1].replace(',','')) if m else None

def extract_formula(raw, annual_header=False, owners=()):
    """Extract evidence, never silently infer tax rates, area conversions or approval."""
    if raw.get('H')=='業主自建案':
        known={'calculationType':'notApplicable','rentRequired':False,
               'reason':'資產表H欄「業主自建案」；客戶2026-11-25確認為業主自有、我方僅建置，無須支付租金。'}
        return {**known,'sourceMissing':[],'sourceWarnings':[],'sourceKnown':dict(known)}
    missing=[]; warnings=[]
    r={'calculationType':'pendingUnknown','basis':{'V':'gross','X':'untaxed'}.get(raw.get('AY')),
       'percent':None,'cycleMonths':{'一個月':1,'二個月':2,'兩個月':2,'三個月':3,'四個月':4,'六個月':6,'一年':12}.get(raw.get('BC'))}
    pct=number(raw.get('BA')); aw=number(raw.get('AW')); annual=annual_amount(raw.get('BB'))
    owner_annual=[annual_amount(o.get('amount')) for o in owners if o.get('unitRate')=='經營年租金']
    if annual is None and len(owner_annual)==1: annual=owner_annual[0]
    if pct is not None: r.update(calculationType='percentage',percent=str(round(pct*100,8)))
    floor=re.fullmatch(r'([\d.]+)%\(含稅低於([\d.]+)萬以含稅\2萬計\)',str(raw.get('BA','')))
    if floor:
        r.update(calculationType='percentageFloor',percent=floor[1],minimumRent=float(floor[2])*10000,minimumBasis='gross',minimumUnit='cycle')
        missing.append('taxPercent')
    if '最低' in str(raw.get('BB','')):
        r.update(calculationType='percentageFloor',minimumRent=None,minimumUnit=None,minimumBasis=None)
        missing.extend(['minimumRent','minimumUnit','minimumBasis'])
    if annual is not None:
        r.update(calculationType='annualReference',annualRent=annual,tolerancePercent=None)
        missing.extend(['annualAdjustmentMethod','tolerancePercent'])
    if aw is not None:
        r.update(calculationType='fixed',fixedAmount=aw,fixedUnit='year' if annual_header else None)
        # An equal per-payment amount and a subannual schedule contradict an annual header.
        numeric_owners=[o for o in owners if number(o.get('amount')) is not None]
        if numeric_owners and r['cycleMonths'] and r['cycleMonths']<12 and sum(o['amount'] for o in numeric_owners)==aw:
            r['fixedUnit']=None; warnings.append('資產表年租金標題與土地表每期金額相同，金額週期互相矛盾。')
        if r['fixedUnit'] is None: missing.append('fixedUnit')
    rate=re.fullmatch(r'([\d.]+)(萬)?/(每)?(公頃|分地)',str(raw.get('AU','')))
    if rate:
        r.update(unitRate=float(rate[1])*(10000 if rate[2] else 1),ratePeriod='year' if annual_header else None,
                 area=number(raw.get('AS')),areaUnit='hectare',rateAreaUnit='hectare' if rate[4]=='公頃' else 'fenLand')
        if aw is None:
            r['calculationType']='areaRate'
            if r['area'] is None: missing.append('area')
            if r['rateAreaUnit']!=r['areaUnit']: missing.append('areaUnitConversion')
            if r['ratePeriod'] is None: missing.append('ratePeriod')
            # Explicit annual amount provides a usable alternative without inventing conversion.
            alternatives=[annual_amount(o.get('amount')) if annual_amount(o.get('amount')) is not None else
                          number(o.get('amount')) if o.get('paymentSchedule') in ('每年','一年') else None for o in owners]
            if len(alternatives)==1 and alternatives[0] is not None:
                r.update(calculationType='fixed',fixedAmount=alternatives[0],fixedUnit='year')
                missing=[m for m in missing if m not in ('area','areaUnitConversion','ratePeriod')]
    if isinstance(raw.get('AW'),str) and re.match(r'^\d+\(',raw['AW']):
        r.update(calculationType='fixed',fixedAmount=float(raw['AW'].split('(')[0]),fixedUnit='year' if annual_header else None)
        warnings.append('金額含當年地價公式；僅保留來源年額，地價更新另行核對。')
    explicit_owner_annual=[annual_amount(o.get('amount')) for o in owners if annual_amount(o.get('amount')) is not None]
    if len(explicit_owner_annual)==1 and r['calculationType']=='pendingUnknown':
        r.update(calculationType='fixed',fixedAmount=explicit_owner_annual[0],fixedUnit='year')
    elif explicit_owner_annual and r['calculationType']=='percentage':
        r['ownerAnnualAmountReferences']=explicit_owner_annual
        missing.append('annualAndPercentageRelationship')
        warnings.append('土地表另有明示年額，須釐清與資產表百分比的關係。')
    if r['calculationType']=='pendingUnknown': missing.append('formula')
    if r['calculationType'] in ('percentage','percentageFloor') and not r['basis']: missing.append('basis')
    tax_raw=raw.get('AV') if r['calculationType'] in ('fixed','areaRate') else raw.get('AZ')
    r.update(taxMode={'內扣':'included','外加':'added'}.get(tax_raw),taxPercent=None,taxSourceValue=tax_raw)
    if not r['cycleMonths']: missing.append('cycleMonths')
    r.update(sourceMissing=list(dict.fromkeys(missing)),sourceWarnings=warnings,
             sourceKnown={k:v for k,v in r.items() if v is not None and k not in ('sourceMissing','sourceWarnings')})
    return r

def identity_name(value):
    """Only normalize typography; never fuzzy-match unrelated project names."""
    return re.sub(r'\s+', '', str(value or '')).replace('（','(').replace('）',')')

def owner_evidence(workbook):
    """Index named project blocks, not reused project codes. Exclude contact/bank data."""
    out=[]; project=None; names=set()
    if '1. 土地所有權人清單' not in workbook.sheetnames: return out
    ws=workbook['1. 土地所有權人清單']
    for row in range(4,ws.max_row+1):
        code=ws.cell(row,3).value
        row_names={identity_name(ws.cell(row,c).value) for c in (4,5,12)}-{''}
        if code or row_names:
            project=str(code or '').strip(); names=row_names
        elif not any(ws.cell(row,c).value is not None for c in range(1,30)):
            project=None; names=set()
        name=ws.cell(row,18).value
        if not names: continue
        out.append({'name':name,'amount':scalar(ws.cell(row,28).value),
            'unitRate':scalar(ws.cell(row,27).value),'paymentSchedule':scalar(ws.cell(row,29).value),
            'sourceSheet':ws.title,'sourceRow':row,'sourceProjectCode':project,
            'projectNames':sorted(names),'payeeInstruction':scalar(ws.cell(row,22).value),
            'noRent':ws.cell(row,28).value=='無付租金'})
    return out

def match_owners(evidence, raw):
    """Names/explicit aliases identify the site; a code alone never authorizes a join."""
    aliases={identity_name(raw.get(c)) for c in ('D','E')} - {''}
    aliases.update(identity_name(v) for v in raw.get('identityAliases',[]) if v)
    matches=[dict(o) for o in evidence if aliases.intersection(o['projectNames'])]
    payee=identity_name(raw.get('W'))
    for o in matches:
        o['matchEvidence']={'method':'exactProjectNameOrAlias',
            'matchedNames':sorted(aliases.intersection(o['projectNames'])),
            'projectCodeMatches':o['sourceProjectCode']==raw.get('C'),
            'payeeCorroborated':bool(payee and (payee==identity_name(o['name']) or payee in identity_name(o['payeeInstruction'])))}
    return matches

def prepare(source, stations, overrides):
    import openpyxl
    digest=hashlib.sha256(source.read_bytes()).hexdigest()
    workbook=openpyxl.load_workbook(source,data_only=True)
    ws=workbook['0. 資產及義務項目清單']; owners=owner_evidence(workbook)
    annual_header=ws['AS1'].value=='土地租金(年)' and any(str(x)=='AS1:AW1' for x in ws.merged_cells.ranges)
    prepared=[];unmatched=[]; seen=set(); all_source_rows=[]
    for r in range(4,ws.max_row+1):
        c=lambda col:scalar(ws[f'{col}{r}'].value)
        project=str(c('C') or '').strip(); key=meter(c('R'))
        self_built=c('H')=='業主自建案'
        if (not project or project=='#N/A') and not self_built: continue
        raw={col:c(col) for col in ['C','D','E','H','R','W','Y','AA','AB','AC','AF','AS','AT','AU','AV','AW','AY','AZ','BA','BB','BC','BD']}
        matched_owners=match_owners(owners,raw)
        all_source_rows.append({'sourceRow':r,'projectCode':project,'meterKey':key,'sourceValues':raw,
            'formula':extract_formula(raw,annual_header,matched_owners),'ownerEvidence':matched_owners})
        if not key:
            unmatched.append({'projectCode':project,'meterKey':key,'row':r,'reason':'來源缺完整電號；公式仍保留在 allSourceRows'});continue
        hits=[s for s in stations if meter(s.get('customerNumber'))==key or key in s.get('meterKeys',[])]
        # A shared meter is insufficient to uniquely select one project/rule.
        if len(hits)!=1 or sum(1 for rr in range(4,ws.max_row+1) if meter(ws[f'R{rr}'].value)==key)!=1:
            unmatched.append({'projectCode':project,'meterKey':key,'row':r,'reason':'電號未唯一對應；不可猜測案場'});continue
        source_key=f'asset-rent:meter:{key}' if self_built and (not project or project=='#N/A') else f'asset-rent:{project}'
        if source_key in seen: raise ValueError('duplicate project '+project)
        seen.add(source_key)
        rule={'status':'pending','calculationType':'percentage' if isinstance(c('BA'),(int,float)) else 'pendingUnknown','payee':c('W'),
              'basis':{'V':'gross','X':'untaxed'}.get(c('AY')),
              'percent':str(round(float(c('BA'))*100,8)) if isinstance(c('BA'),(int,float)) else None,
              'cycleMonths':{'一個月':1,'三個月':3,'六個月':6,'一年':12}.get(c('BC')),
              'validFrom':None,'validTo':None,
              'ruleSource':f'{source.name}／{ws.title} 第{r}列',
              'sourceKey':source_key,'sourceSha256':digest,'sourceRow':r,'sourceProjectCode':project,
              'sourceValues':raw,'leaseEvidence':{'status':'missing','attachmentIds':[]},
              'notes':['資產總表來源已建檔；未確認參數保留待確認，不自動啟用。','簽署日期不是計算規則生效日期；原值保留於來源。']}
        rule.update(extract_formula(raw,annual_header,matched_owners))
        rule['sourceOwnerEvidence']=matched_owners
        label=str(c('W') or '')
        group=re.search(r'([二兩三四五六七八九十\d]+)人',label)
        if group:
            rule['payeeGroupLabel']=label
            rule['payeeGroupCount']={'二':2,'兩':2,'三':3,'四':4,'五':5,'六':6,'七':7,'八':8,'九':9,'十':10}.get(group[1],int(group[1]) if group[1].isdigit() else None)
            paid=[o for o in matched_owners if not o.get('noRent') and number(o.get('amount')) is not None]
            # Only expose an allocation when every named member is sourced and the
            # per-payment total reconciles exactly to the source annual amount.
            months=rule.get('cycleMonths')
            schedules={o.get('paymentSchedule') for o in paid}
            if (len(paid)==rule['payeeGroupCount'] and months and rule.get('fixedUnit')=='year'
                    and schedules=={'1,7月15號前'} and months==6
                    and sum(o['amount'] for o in paid)*2==rule.get('fixedAmount')):
                rule['partyGroup']={'label':label,'count':len(paid),
                    'members':[{'name':o['name'],'amount':o['amount'],'share':f"每半年 {o['amount']:,.0f} 元"} for o in paid],
                    'allocationStatus':'confirmed','periodMonths':months,'note':'依土地所有權人表每期金額分配，非平均分攤；不付租金列另保留於來源證據。'}
        if annual_header: rule['sourceUnitEvidence']='AS1:AW1 合併標題：土地租金(年)；AU3：年租金率'
        override=overrides.get(project+'|'+key)
        if override: rule.update(override)
        prepared.append({'stationId':hits[0]['id'],'stationName':hits[0]['name'],'customerNumber':c('R'),'rule':rule})
    # Shared-group references never duplicate a group amount onto member stations.
    previous=None
    for item in prepared:
        rule=item['rule']; text=str(rule['sourceValues'].get('AW') or '')
        if any(term in text for term in ('合併計算','共計')) and previous:
            anchor=previous['rule']
            if anchor.get('payee')==rule.get('payee'):
                key=anchor.get('sharedGroupKey',anchor['sourceKey'])
                anchor.update(sharedGroupKey=key,groupAllocationMissing=True)
                rule.update(sharedGroupKey=key,groupAllocationMissing=True,calculationType='fixed',
                            fixedAmount=anchor.get('fixedAmount'),fixedUnit=anchor.get('fixedUnit'),sharedAmountReferenceOnly=True)
                rule['sourceMissing']=[x for x in rule['sourceMissing'] if x!='formula']
                for target in (anchor,rule):
                    if 'sharedGroupAllocation' not in target['sourceMissing']: target['sourceMissing'].append('sharedGroupAllocation')
        previous=item
    counts={}
    previous_source=None
    for entry in all_source_rows:
        text=str(entry['sourceValues'].get('AW') or '')
        if previous_source and any(term in text for term in ('合併計算','共計')) and previous_source['sourceValues'].get('W')==entry['sourceValues'].get('W'):
            anchor=previous_source['formula']; member=entry['formula']
            group=anchor.get('sharedGroupKey','asset-rent:'+previous_source['projectCode'])
            member.update(calculationType='fixed',fixedAmount=anchor.get('fixedAmount'),fixedUnit=anchor.get('fixedUnit'),sharedAmountReferenceOnly=True)
            for formula in (anchor,member):
                formula.update(sharedGroupKey=group,groupAllocationMissing=True)
                formula['sourceMissing']=[x for x in formula['sourceMissing'] if x!='formula']
                if 'sharedGroupAllocation' not in formula['sourceMissing']: formula['sourceMissing'].append('sharedGroupAllocation')
        previous_source=entry
    for item in prepared:
        kind=item['rule']['calculationType']; counts[kind]=counts.get(kind,0)+1
    return {'sourceFile':source.name,'sourceSha256':digest,'items':prepared,'unmatched':unmatched,'formulaCounts':counts,'allSourceRows':all_source_rows}

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--asset',required=True,type=Path);p.add_argument('--stations',required=True,type=Path);p.add_argument('--overrides',required=True,type=Path);p.add_argument('--out',required=True,type=Path)
    a=p.parse_args();result=prepare(a.asset,json.loads(a.stations.read_text(encoding='utf-8-sig')),json.loads(a.overrides.read_text(encoding='utf-8-sig')))
    a.out.parent.mkdir(parents=True,exist_ok=True);a.out.write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf8')
    print(json.dumps({'prepared':len(result['items']),'confirmed':sum(i['rule']['status']=='confirmed' for i in result['items']),'unmatched':len(result['unmatched'])}))
