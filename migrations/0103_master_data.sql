-- 卡 A（資料模型重整）：一次建好案場擴欄、案場↔電號一對多、受款對象擴欄、地主↔案場分配、
-- 合約擴欄、附件 contract_id。只加不改舊檔（0100_energy.sql／0102_energy_intake.sql 不動）。
-- `tbl_energy_station.customer_number`／`landowner_id` 保留不動：收件比對、流程主體、總覽頁都靠它們。

-- ── 案場擴欄 ──
ALTER TABLE public.tbl_energy_station
  ADD COLUMN IF NOT EXISTS project_code character varying(20),
  ADD COLUMN IF NOT EXISTS short_name character varying(100),
  ADD COLUMN IF NOT EXISTS region character varying(100),
  ADD COLUMN IF NOT EXISTS category character varying(100),
  ADD COLUMN IF NOT EXISTS site_type character varying(20),
  ADD COLUMN IF NOT EXISTS spv_payee_id bigint,
  ADD COLUMN IF NOT EXISTS service_district character varying(100),
  ADD COLUMN IF NOT EXISTS capacity_kw numeric(12,3),
  ADD COLUMN IF NOT EXISTS grid_date date,
  ADD COLUMN IF NOT EXISTS serial_no integer,
  ADD COLUMN IF NOT EXISTS enabled boolean DEFAULT true NOT NULL;

ALTER TABLE public.tbl_energy_station
  ADD CONSTRAINT tbl_energy_station_spv_payee_id_fkey FOREIGN KEY (spv_payee_id)
    REFERENCES public.tbl_energy_landowner(id) ON UPDATE CASCADE ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS energy_station_project_code ON public.tbl_energy_station USING btree (organization_id, project_code);

-- ── 案場↔電號一對多 ──
CREATE TABLE IF NOT EXISTS public.tbl_energy_station_meter (
    id bigserial NOT NULL,
    organization_id bigint NOT NULL,
    station_id bigint NOT NULL,
    customer_number character varying(60) NOT NULL,
    customer_number_key character varying(60),
    usage character varying(20) DEFAULT 'wholesale'::character varying NOT NULL,
    taipower_contract_no character varying(80),
    feed_in_rate numeric(10,4),
    green_rate numeric(10,4),
    valid_from date,
    valid_to date,
    note text,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone,
    CONSTRAINT tbl_energy_station_meter_pkey PRIMARY KEY (id)
);

ALTER TABLE ONLY public.tbl_energy_station_meter
  ADD CONSTRAINT tbl_energy_station_meter_organization_id_fkey FOREIGN KEY (organization_id)
    REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE,
  ADD CONSTRAINT tbl_energy_station_meter_station_id_fkey FOREIGN KEY (station_id)
    REFERENCES public.tbl_energy_station(id) ON UPDATE CASCADE ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS energy_station_meter_org_station ON public.tbl_energy_station_meter USING btree (organization_id, station_id);
CREATE INDEX IF NOT EXISTS energy_station_meter_org_key ON public.tbl_energy_station_meter USING btree (organization_id, customer_number_key);

-- 正規化 trigger：寫法照 0102_energy_intake.sql 的 eo_station_customer_number_key，套在 meter 表。
CREATE OR REPLACE FUNCTION eo_station_meter_customer_number_key() RETURNS trigger AS $$
BEGIN
    NEW.customer_number_key := NULLIF(upper(regexp_replace(COALESCE(NEW.customer_number, ''), '[^0-9A-Za-z]', '', 'g')), '');
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS eo_station_meter_customer_number_key ON public.tbl_energy_station_meter;
CREATE TRIGGER eo_station_meter_customer_number_key
    BEFORE INSERT OR UPDATE OF customer_number ON public.tbl_energy_station_meter
    FOR EACH ROW EXECUTE FUNCTION eo_station_meter_customer_number_key();

-- backfill：每個現有案場用它的 customer_number 建一列 wholesale。
INSERT INTO public.tbl_energy_station_meter (organization_id, station_id, customer_number, usage, valid_from)
SELECT s.organization_id, s.id, s.customer_number, 'wholesale', NULL
  FROM public.tbl_energy_station s
 WHERE s.deleted_at IS NULL
   AND NOT EXISTS (
     SELECT 1 FROM public.tbl_energy_station_meter m WHERE m.station_id = s.id AND m.deleted_at IS NULL
   );

-- ── 受款對象擴欄（沿用 tbl_energy_landowner，不改表名）──
ALTER TABLE public.tbl_energy_landowner
  ADD COLUMN IF NOT EXISTS party_type character varying(20) DEFAULT 'individual'::character varying NOT NULL,
  ADD COLUMN IF NOT EXISTS code character varying(10),
  ADD COLUMN IF NOT EXISTS address character varying(300);

CREATE INDEX IF NOT EXISTS energy_landowner_org_party_type ON public.tbl_energy_landowner USING btree (organization_id, party_type);
CREATE UNIQUE INDEX IF NOT EXISTS energy_landowner_org_code ON public.tbl_energy_landowner USING btree (organization_id, code) WHERE (code IS NOT NULL AND deleted_at IS NULL);

-- ── 地主↔案場多對多＋分配 ──
CREATE TABLE IF NOT EXISTS public.mn_energy_landowner_station (
    id bigserial NOT NULL,
    organization_id bigint NOT NULL,
    landowner_id bigint NOT NULL,
    station_id bigint NOT NULL,
    method character varying(20) DEFAULT 'ratio'::character varying NOT NULL,
    ratio numeric(7,4),
    fixed_amount numeric(14,2),
    land_area numeric(12,2),
    valid_from date,
    valid_to date,
    change_reason text,
    note text,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone,
    CONSTRAINT mn_energy_landowner_station_pkey PRIMARY KEY (id)
);

ALTER TABLE ONLY public.mn_energy_landowner_station
  ADD CONSTRAINT mn_energy_landowner_station_organization_id_fkey FOREIGN KEY (organization_id)
    REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE,
  ADD CONSTRAINT mn_energy_landowner_station_landowner_id_fkey FOREIGN KEY (landowner_id)
    REFERENCES public.tbl_energy_landowner(id) ON UPDATE CASCADE ON DELETE CASCADE,
  ADD CONSTRAINT mn_energy_landowner_station_station_id_fkey FOREIGN KEY (station_id)
    REFERENCES public.tbl_energy_station(id) ON UPDATE CASCADE ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS energy_landowner_station_org_station ON public.mn_energy_landowner_station USING btree (organization_id, station_id);
CREATE INDEX IF NOT EXISTS energy_landowner_station_org_landowner ON public.mn_energy_landowner_station USING btree (organization_id, landowner_id);

-- backfill：每個現有案場的 landowner_id 建一列 ratio=100（只存，不算錢）。
INSERT INTO public.mn_energy_landowner_station (organization_id, landowner_id, station_id, method, ratio, valid_from)
SELECT s.organization_id, s.landowner_id, s.id, 'ratio', 100, NULL
  FROM public.tbl_energy_station s
 WHERE s.deleted_at IS NULL
   AND NOT EXISTS (
     SELECT 1 FROM public.mn_energy_landowner_station a WHERE a.station_id = s.id AND a.deleted_at IS NULL
   );

-- ── 合約擴欄（本卡只建欄，端點與畫面在下一張卡）──
ALTER TABLE public.tbl_energy_contract
  ADD COLUMN IF NOT EXISTS contract_type character varying(60),
  ADD COLUMN IF NOT EXISTS signed_on date,
  ADD COLUMN IF NOT EXISTS renewal_remind_on date,
  ADD COLUMN IF NOT EXISTS terms_version character varying(40),
  ADD COLUMN IF NOT EXISTS payment_frequency character varying(20),
  ADD COLUMN IF NOT EXISTS payment_terms text;

-- ── 附件 contract_id（本卡只建欄；Owner::Contract 與上傳端點在下一張卡）──
ALTER TABLE public.tbl_energy_attachment
  ADD COLUMN IF NOT EXISTS contract_id bigint;

ALTER TABLE public.tbl_energy_attachment
  ADD CONSTRAINT tbl_energy_attachment_contract_id_fkey FOREIGN KEY (contract_id)
    REFERENCES public.tbl_energy_contract(id) ON UPDATE CASCADE ON DELETE CASCADE;

ALTER TABLE public.tbl_energy_attachment DROP CONSTRAINT IF EXISTS energy_attachment_owner_chk;
ALTER TABLE public.tbl_energy_attachment ADD CONSTRAINT energy_attachment_owner_chk
  CHECK (((execution_id IS NOT NULL) OR (ingest_batch_id IS NOT NULL) OR (contract_id IS NOT NULL)));
