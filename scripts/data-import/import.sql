-- Run with psql -v org_id=... -v apply=false -f import.sql, from beside data.sql.
-- Default rollback; no seeded IDs, no OCR/payment/workflow rows changed.
\set ON_ERROR_STOP on
\if :{?org_id}
\else
  \echo 'org_id is required; no writes performed'
  \quit 2
\endif
\if :{?apply}
\else
  \set apply false
\endif
BEGIN;
SET LOCAL lock_timeout='10s';
SELECT set_config('energy_import.org_id', :'org_id', true);
CREATE TEMP TABLE import_payload (payload jsonb) ON COMMIT DROP;
\ir data.sql
-- An append-only source ledger. It preserves unresolved rows without activating a formula.
CREATE TABLE IF NOT EXISTS public.tbl_energy_source_import_row (
  organization_id bigint NOT NULL REFERENCES public.tbl_organization(id),
  source_key text NOT NULL,
  kind text NOT NULL,
  payload jsonb NOT NULL,
  imported_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (organization_id,source_key)
);
CREATE TEMP TABLE import_report (kind text, source_key text, station_id bigint, result text) ON COMMIT DROP;
DO $import$
DECLARE
 org bigint:=current_setting('energy_import.org_id')::bigint;
 r jsonb; row_data jsonb; sid bigint; ids bigint[]; meter_ids bigint[]; plant_ids bigint[];
 owner_id bigint; owner_ids bigint[]; old_data jsonb; rules jsonb; invoice jsonb; kv record;
BEGIN
 IF NOT EXISTS(SELECT 1 FROM public.tbl_organization WHERE id=org) THEN RAISE EXCEPTION 'organization not found'; END IF;
 -- Serializes imports of this tool and prevents races with platform master edits.
 LOCK TABLE public.tbl_energy_station,public.tbl_energy_station_meter,public.tbl_energy_landowner IN SHARE ROW EXCLUSIVE MODE;
 FOR r IN SELECT value FROM import_payload,jsonb_array_elements(payload->'rows') LOOP
   INSERT INTO public.tbl_energy_source_import_row(organization_id,source_key,kind,payload)
     VALUES(org,r->>'sourceKey',r->>'kind',r) ON CONFLICT DO NOTHING;
 END LOOP;
 FOR r IN SELECT value FROM import_payload,jsonb_array_elements(payload->'stations') LOOP
   SELECT array_agg(DISTINCT id) INTO meter_ids FROM (
     SELECT id FROM public.tbl_energy_station WHERE organization_id=org AND deleted_at IS NULL
       AND regexp_replace(customer_number,'[^0-9]','','g')=r->>'meterKey'
     UNION
     SELECT s.id FROM public.tbl_energy_station_meter m JOIN public.tbl_energy_station s ON s.id=m.station_id
       WHERE m.organization_id=org AND s.organization_id=org AND m.deleted_at IS NULL AND s.deleted_at IS NULL
       AND regexp_replace(m.customer_number,'[^0-9]','','g')=r->>'meterKey'
   ) matched;
   SELECT array_agg(id) INTO plant_ids FROM public.tbl_energy_station
     WHERE organization_id=org AND deleted_at IS NULL AND NULLIF(data->>'plantNo','')=NULLIF(r->>'plantNo','');
   SELECT array_agg(DISTINCT id) INTO ids FROM unnest(COALESCE(meter_ids,'{}'::bigint[])||COALESCE(plant_ids,'{}'::bigint[])) id;
   IF COALESCE(cardinality(ids),0)>1 THEN
     INSERT INTO import_report VALUES('station',r->>'sourceKey',NULL,'conflict: meter and plant number are not unique; skipped'); CONTINUE;
   END IF;
   sid:=ids[1];
   IF sid IS NULL THEN
     SELECT array_agg(id) INTO owner_ids FROM public.tbl_energy_landowner
       WHERE organization_id=org AND deleted_at IS NULL AND party_type='subsidiary' AND name=r->>'spv';
     IF COALESCE(cardinality(owner_ids),0)>1 THEN
       INSERT INTO import_report VALUES('station',r->>'sourceKey',NULL,'conflict: duplicate subsidiary; skipped');CONTINUE;
     END IF;
     owner_id:=owner_ids[1];
     IF owner_id IS NULL THEN
       INSERT INTO public.tbl_energy_landowner(organization_id,name,party_type,status)
         VALUES(org,r->>'spv','subsidiary','active') RETURNING id INTO owner_id;
     END IF;
     INSERT INTO public.tbl_energy_station(organization_id,name,customer_number,landowner_id,spv_payee_id,region,capacity_kw,data)
       VALUES(org,r->>'name',r->>'customerNumber',owner_id,owner_id,r->>'region',NULLIF(r->>'capacityKw','')::numeric,
         jsonb_build_object('plantNo',r->>'plantNo','landownerAssignmentPending',true)) RETURNING id INTO sid;
     -- Required legacy owner column uses the owning subsidiary, as in the platform importer.
     -- No fictitious 100% landlord payout allocation or signed contract is created.
     INSERT INTO import_report VALUES('station',r->>'sourceKey',sid,'created');
   ELSE
     INSERT INTO import_report VALUES('station',r->>'sourceKey',sid,'matched: existing fields preserved');
   END IF;
   IF NOT EXISTS(SELECT 1 FROM public.tbl_energy_station_meter WHERE organization_id=org AND station_id=sid AND deleted_at IS NULL
       AND regexp_replace(customer_number,'[^0-9]','','g')=r->>'meterKey') THEN
     INSERT INTO public.tbl_energy_station_meter(organization_id,station_id,customer_number,usage)
       VALUES(org,sid,r->>'customerNumber','wholesale');
   END IF;
   SELECT data INTO old_data FROM public.tbl_energy_station WHERE id=sid AND organization_id=org;
   invoice:=COALESCE(NULLIF(old_data->'invoice','null'::jsonb),'{}'::jsonb);
   FOR kv IN SELECT key,value FROM jsonb_each(r->'invoice') LOOP
     IF (NOT invoice?kv.key OR invoice->kv.key='null'::jsonb OR invoice->>kv.key='')
       AND kv.value<>'null'::jsonb AND kv.value#>>'{}' NOT IN ('','-','N/A') THEN
       invoice:=jsonb_set(invoice,ARRAY[kv.key],kv.value,true);
     END IF;
   END LOOP;
   SELECT COALESCE(jsonb_agg(value),'[]'::jsonb) INTO rules FROM import_payload,jsonb_array_elements(payload->'rules')
     WHERE value->>'meterKey'=r->>'meterKey';
   -- Source metadata only; do not overwrite rentPaymentDrafts, rentCycleDraft or published strategies.
   row_data:=old_data||jsonb_build_object('invoice',invoice);
   row_data:=jsonb_set(row_data,'{sourceImports}',COALESCE(old_data->'sourceImports','{}'::jsonb)||
     jsonb_build_object(r->>'sourceKey',jsonb_build_object('station',r,'rentalSources',rules)),true);
   UPDATE public.tbl_energy_station SET data=row_data WHERE id=sid AND organization_id=org AND data IS DISTINCT FROM row_data;
 END LOOP;
END $import$;
SELECT * FROM import_report ORDER BY station_id,source_key;
SELECT kind,count(*) AS preserved_source_rows FROM public.tbl_energy_source_import_row WHERE organization_id=:'org_id'::bigint GROUP BY kind;
\if :apply
 COMMIT;
 \echo 'Import committed; source rules are NOT activated calculation strategies.'
\else
 ROLLBACK;
 \echo 'Dry-run rolled back; no records saved (sequence IDs may have advanced).'
\endif
