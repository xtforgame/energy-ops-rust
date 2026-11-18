-- 每一關「必傳附件類別」「必填欄位」的定義與送出前檢查（20261118-0811-d）。

-- ── 流程節點：這一關的要求（空＝沒有要求，絕大多數節點都是空） ──
ALTER TABLE public.tbl_energy_workflow_node
  ADD COLUMN IF NOT EXISTS requirements jsonb DEFAULT '{}'::jsonb NOT NULL;

-- ── 附件：類別（封閉代碼，見 eo_domain::attachment_limits::AttachmentCategory） ──
ALTER TABLE public.tbl_energy_attachment
  ADD COLUMN IF NOT EXISTS category character varying(40);
