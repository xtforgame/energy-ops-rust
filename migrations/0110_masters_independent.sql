-- 卡 20261124-2316-masters-independent-unknown-meter：案場、地主、合約各自獨立——
-- 案場可以沒有電號、沒有地主就建檔；合約可以掛多位地主。
-- 只放寬 NOT NULL、新增關聯表＋backfill，不改既有欄位型別、不刪資料。

-- ── 案場：電號、地主改選填 ──
ALTER TABLE public.tbl_energy_station ALTER COLUMN customer_number DROP NOT NULL;
ALTER TABLE public.tbl_energy_station ALTER COLUMN landowner_id DROP NOT NULL;

-- ── 合約：地主改選填（多對多關聯表另建）──
ALTER TABLE public.tbl_energy_contract ALTER COLUMN landowner_id DROP NOT NULL;

-- ── 合約↔地主多對多 ──
CREATE TABLE IF NOT EXISTS public.mn_energy_contract_landowner (
    id bigserial NOT NULL,
    contract_id bigint NOT NULL,
    landowner_id bigint NOT NULL,
    valid_from date,
    valid_to date,
    note text,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone,
    CONSTRAINT mn_energy_contract_landowner_pkey PRIMARY KEY (id)
);

ALTER TABLE ONLY public.mn_energy_contract_landowner
  ADD CONSTRAINT mn_energy_contract_landowner_contract_id_fkey FOREIGN KEY (contract_id)
    REFERENCES public.tbl_energy_contract(id) ON UPDATE CASCADE ON DELETE CASCADE,
  ADD CONSTRAINT mn_energy_contract_landowner_landowner_id_fkey FOREIGN KEY (landowner_id)
    REFERENCES public.tbl_energy_landowner(id) ON UPDATE CASCADE ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS energy_contract_landowner_contract ON public.mn_energy_contract_landowner USING btree (contract_id);
CREATE INDEX IF NOT EXISTS energy_contract_landowner_landowner ON public.mn_energy_contract_landowner USING btree (landowner_id);

-- backfill：每份既有合約用它原本的 landowner_id 建一列（當「第一位地主」）。
INSERT INTO public.mn_energy_contract_landowner (contract_id, landowner_id, valid_from, valid_to)
SELECT c.id, c.landowner_id, c.valid_from, c.valid_to
  FROM public.tbl_energy_contract c
 WHERE c.deleted_at IS NULL
   AND c.landowner_id IS NOT NULL
   AND NOT EXISTS (
     SELECT 1 FROM public.mn_energy_contract_landowner l WHERE l.contract_id = c.id AND l.deleted_at IS NULL
   );
