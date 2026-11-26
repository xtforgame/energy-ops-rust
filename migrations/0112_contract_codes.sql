-- 卡 20261125-2334-contract-codes-workflow-bind：合約綁電費計算代號／租金算法代號／付款方式代號，
-- 存檔時依代號替掛著的案場建工作流綁定。本卡只建欄＋CHECK，端點核值與畫面在 contracts.rs／ContractsPage.tsx。

ALTER TABLE public.tbl_energy_contract
  ADD COLUMN IF NOT EXISTS bill_codes text[] NOT NULL DEFAULT '{}',
  ADD COLUMN IF NOT EXISTS rent_calc_code character varying(8),
  ADD COLUMN IF NOT EXISTS rent_pay_code character varying(8);

ALTER TABLE public.tbl_energy_contract
  DROP CONSTRAINT IF EXISTS tbl_energy_contract_bill_codes_chk,
  ADD CONSTRAINT tbl_energy_contract_bill_codes_chk
    CHECK (bill_codes <@ ARRAY['E-A','E-B','E-C','E-D']::text[]);

ALTER TABLE public.tbl_energy_contract
  DROP CONSTRAINT IF EXISTS tbl_energy_contract_rent_calc_code_chk,
  ADD CONSTRAINT tbl_energy_contract_rent_calc_code_chk
    CHECK (rent_calc_code IS NULL OR rent_calc_code IN ('R-0','R-A','R-B','R-C','R-D'));

ALTER TABLE public.tbl_energy_contract
  DROP CONSTRAINT IF EXISTS tbl_energy_contract_rent_pay_code_chk,
  ADD CONSTRAINT tbl_energy_contract_rent_pay_code_chk
    CHECK (rent_pay_code IS NULL OR rent_pay_code IN ('RP-1','RP-2','RP-3','RP-4'));
