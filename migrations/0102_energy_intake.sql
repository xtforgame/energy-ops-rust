-- 收件與辨識需要的補充（App 5 由 model hook／onInitDatabase 做的事，這裡改成 DB 端保證）。

-- 案場電號的正規化值：電費單配案場比的是它（`01-2233-4455` 與 `0122334455` 是同一個）。
-- 寫入與改電號時由 trigger 維護，主檔不管從哪條路進來都不會漏算。
CREATE OR REPLACE FUNCTION eo_station_customer_number_key() RETURNS trigger AS $$
BEGIN
    NEW.customer_number_key := NULLIF(upper(regexp_replace(COALESCE(NEW.customer_number, ''), '[^0-9A-Za-z]', '', 'g')), '');
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS eo_station_customer_number_key ON public.tbl_energy_station;
CREATE TRIGGER eo_station_customer_number_key
    BEFORE INSERT OR UPDATE OF customer_number ON public.tbl_energy_station
    FOR EACH ROW EXECUTE FUNCTION eo_station_customer_number_key();

UPDATE public.tbl_energy_station
   SET customer_number_key = NULLIF(upper(regexp_replace(customer_number, '[^0-9A-Za-z]', '', 'g')), '')
 WHERE customer_number_key IS NULL;

CREATE INDEX IF NOT EXISTS energy_station_org_customer_key ON public.tbl_energy_station USING btree (organization_id, customer_number_key);

-- 對帳背景任務只看這兩種狀態的批次。
CREATE INDEX IF NOT EXISTS energy_ingest_batch_status ON public.tbl_energy_ingest_batch USING btree (status, id) WHERE (deleted_at IS NULL);
