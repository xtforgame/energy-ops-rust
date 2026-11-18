-- 卡 B（20261118-0810-b）：電費單明細 19 欄（整併版「資料庫欄位字典」DB-088～DB-107 扣主鍵，
-- 加「契約編號」「區處」兩個開卡句點名的）＋比對結果 2 欄（contract_id、station_meter_id）。
-- 19 欄合併成一個 jsonb `detail_fields`（鍵是 camelCase 欄位名，驅動表見
-- `eo_domain::taipower_extractor::BILL_DETAIL_FIELDS`），不開 19 支具名欄——
-- 同一條欄位擴充鏈只用一個機制（commander 2026-11-18 規模核定時的提醒）。
-- 只加不改舊檔；變更檔只用這一個（commander 禁止清單）。

ALTER TABLE public.tbl_energy_bill
  ADD COLUMN IF NOT EXISTS detail_fields jsonb DEFAULT '{}'::jsonb NOT NULL,
  ADD COLUMN IF NOT EXISTS contract_id bigint,
  ADD COLUMN IF NOT EXISTS station_meter_id bigint;

ALTER TABLE public.tbl_energy_bill
  ADD CONSTRAINT tbl_energy_bill_contract_id_fkey FOREIGN KEY (contract_id)
    REFERENCES public.tbl_energy_contract(id) ON UPDATE CASCADE ON DELETE SET NULL;

ALTER TABLE public.tbl_energy_bill
  ADD CONSTRAINT tbl_energy_bill_station_meter_id_fkey FOREIGN KEY (station_meter_id)
    REFERENCES public.tbl_energy_station_meter(id) ON UPDATE CASCADE ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS energy_bill_contract_id ON public.tbl_energy_bill USING btree (contract_id);
