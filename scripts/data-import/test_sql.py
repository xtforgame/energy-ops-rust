"""Isolated importer check. Uses an already installed postgres:15-alpine image; no platform database access."""
import json,subprocess,sys,tempfile,time,uuid
from pathlib import Path

docker=sys.argv[1] if len(sys.argv)>1 else 'docker'
name='energy-import-test-'+uuid.uuid4().hex[:8]
def run(*args):
    r=subprocess.run([docker,*args],capture_output=True,text=True,encoding='utf8')
    if r.returncode:raise RuntimeError(r.stderr+r.stdout)
    return r.stdout.strip()
def query(sql):return run('exec',name,'psql','-U','postgres','-At','-c',sql)
schema='''
CREATE TABLE tbl_organization(id bigint PRIMARY KEY); INSERT INTO tbl_organization VALUES(1),(2);
CREATE TABLE tbl_energy_landowner(id bigserial PRIMARY KEY,organization_id bigint,name text,party_type text,status text,deleted_at timestamptz);
CREATE TABLE tbl_energy_station(id bigserial PRIMARY KEY,organization_id bigint,name text,customer_number text,landowner_id bigint,spv_payee_id bigint,region text,capacity_kw numeric,data jsonb DEFAULT '{}',deleted_at timestamptz);
CREATE TABLE tbl_energy_station_meter(id bigserial PRIMARY KEY,organization_id bigint,station_id bigint,customer_number text,usage text,deleted_at timestamptz);
INSERT INTO tbl_energy_station(organization_id,name,customer_number,data) VALUES(1,'existing','00123456789','{"plantNo":"A","invoice":{"contact":"keep"},"rentPaymentDrafts":[{"keep":true}]}');
INSERT INTO tbl_energy_station(organization_id,name,customer_number,data) VALUES(2,'other-org','00123456789','{}');
'''
payload={'stations':[{'meterKey':'00123456789','customerNumber':'00-12-3456-78-9','plantNo':'A','name':'source alias','spv':'test co','sourceKey':'a','invoice':{'contact':'replace','email1':'new@example.test'}},{'meterKey':'00123456780','customerNumber':'00123456780','plantNo':'B','name':'new','spv':'test co','sourceKey':'b','invoice':{}}],
 'rules':[{'meterKey':'00123456789','status':'pending','automaticPaymentAllowed':False}],
 'rows':[{'sourceKey':'a','kind':'station_source','cells':{'test':'value'}},{'sourceKey':'r','kind':'asset_source','status':'pending'}]}
try:
 run('run','-d','--pull','never','--network','none','--name',name,'-e','POSTGRES_HOST_AUTH_METHOD=trust','postgres:15-alpine')
 for _ in range(30):
  try:query('select 1');break
  except RuntimeError:time.sleep(.5)
 else:raise RuntimeError('fixture postgres unavailable')
 query(schema)
 with tempfile.TemporaryDirectory() as tmp:
  p=Path(tmp);(p/'data.sql').write_text("INSERT INTO import_payload VALUES ('"+json.dumps(payload).replace("'","''")+"'::jsonb);",encoding='utf8')
  run('cp',str(p/'data.sql'),name+':/tmp/data.sql')
  run('cp',str(Path(__file__).with_name('import.sql')),name+':/tmp/import.sql')
  def execute(apply):return run('exec',name,'psql','-U','postgres','-v','org_id=1','-v','apply='+apply,'-f','/tmp/import.sql')
  execute('false');assert query('select count(*) from tbl_energy_station')=='2'
  execute('true');assert query('select count(*) from tbl_energy_station')=='3'
  execute('true');assert query('select count(*) from tbl_energy_station')=='3'
  assert query('select count(*) from tbl_energy_source_import_row')=='2'
  assert query('select count(*) from tbl_energy_station_meter')=='2'
  assert query("select data->'invoice'->>'contact' from tbl_energy_station where id=1")=='keep'
  assert query("select data->'invoice'->>'email1' from tbl_energy_station where id=1")=='new@example.test'
  assert query("select data->'rentPaymentDrafts'->0->>'keep' from tbl_energy_station where id=1")=='true'
  assert query("select data::text from tbl_energy_station where organization_id=2")=='{}'
  # Conflicting meter-vs-plant match must not remap either station.
  payload['stations'][0]['plantNo']='B'
  (p/'data.sql').write_text("INSERT INTO import_payload VALUES ('"+json.dumps(payload).replace("'","''")+"'::jsonb);",encoding='utf8')
  run('cp',str(p/'data.sql'),name+':/tmp/data.sql')
  assert 'conflict: meter and plant number' in execute('true')
  if len(sys.argv)>2:
   package=Path(sys.argv[2]); expected=json.loads((package/'manifest.json').read_text(encoding='utf8'))['counts']
   query('INSERT INTO tbl_organization VALUES(3)')
   run('cp',str(package/'data.sql'),name+':/tmp/data.sql')
   for _ in range(2):
    run('exec',name,'psql','-U','postgres','-v','org_id=3','-v','apply=true','-f','/tmp/import.sql')
   assert query('select count(*) from tbl_energy_station where organization_id=3')==str(expected['stations'])
   assert query('select count(*) from tbl_energy_source_import_row where organization_id=3')==str(expected['sourceRows'])
   assert query('select count(*) from tbl_energy_station_meter where organization_id=3')==str(expected['stations'])
   print('PASS: supplied package imported twice without duplicate stations/meters/source rows (isolated fixture DB only)')
 print('PASS: rollback, repeated import, source deduplication, preserve values/drafts, org isolation, conflicting mapping skip')
finally:
 subprocess.run([docker,'rm','-f',name],capture_output=True)
