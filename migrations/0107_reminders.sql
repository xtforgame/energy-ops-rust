-- 卡 E（提醒機制）：預定事件補「實際發生」「上次提醒」；新表規則參數、假日表、系統內通知。
-- 規則參數表全部允許 NULL（空值＝「未設定」＝不發），不猜任何客戶規則值。

-- ── 預定事件：記實際發生日、上次提醒日 ──
ALTER TABLE public.tbl_energy_calendar_event
  ADD COLUMN IF NOT EXISTS occurred_on date,
  ADD COLUMN IF NOT EXISTS last_notified_on date;

-- ── 提醒規則參數（每組織每種 kind 一列；全部預設空值＝未設定）──
CREATE TABLE IF NOT EXISTS public.tbl_energy_reminder_rule (
    id bigserial NOT NULL,
    organization_id bigint NOT NULL,
    kind character varying(40) NOT NULL,
    lead_days integer,
    repeat_days integer,
    business_days boolean DEFAULT false NOT NULL,
    day_of_month integer,
    enabled boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone,
    CONSTRAINT tbl_energy_reminder_rule_pkey PRIMARY KEY (id)
);

ALTER TABLE ONLY public.tbl_energy_reminder_rule
  ADD CONSTRAINT tbl_energy_reminder_rule_organization_id_fkey FOREIGN KEY (organization_id)
    REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

CREATE UNIQUE INDEX IF NOT EXISTS energy_reminder_rule_org_kind
  ON public.tbl_energy_reminder_rule USING btree (organization_id, kind) WHERE (deleted_at IS NULL);

-- ── 工作天／國定假日表（補班日 is_workday = true）──
CREATE TABLE IF NOT EXISTS public.tbl_energy_holiday (
    id bigserial NOT NULL,
    organization_id bigint NOT NULL,
    day date NOT NULL,
    name character varying(200),
    is_workday boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone,
    CONSTRAINT tbl_energy_holiday_pkey PRIMARY KEY (id)
);

ALTER TABLE ONLY public.tbl_energy_holiday
  ADD CONSTRAINT tbl_energy_holiday_organization_id_fkey FOREIGN KEY (organization_id)
    REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

CREATE UNIQUE INDEX IF NOT EXISTS energy_holiday_org_day
  ON public.tbl_energy_holiday USING btree (organization_id, day) WHERE (deleted_at IS NULL);

-- ── 系統內通知（組織內共用，不指定收件人；Q-14 之前不寄信）──
CREATE TABLE IF NOT EXISTS public.tbl_energy_notification (
    id bigserial NOT NULL,
    organization_id bigint NOT NULL,
    event_id bigint NOT NULL,
    kind character varying(20) NOT NULL,
    message text NOT NULL,
    notify_on date NOT NULL,
    read_at timestamp with time zone,
    read_by bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone,
    CONSTRAINT tbl_energy_notification_pkey PRIMARY KEY (id)
);

ALTER TABLE ONLY public.tbl_energy_notification
  ADD CONSTRAINT tbl_energy_notification_organization_id_fkey FOREIGN KEY (organization_id)
    REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE,
  ADD CONSTRAINT tbl_energy_notification_event_id_fkey FOREIGN KEY (event_id)
    REFERENCES public.tbl_energy_calendar_event(id) ON UPDATE CASCADE ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS energy_notification_org_unread
  ON public.tbl_energy_notification USING btree (organization_id, read_at) WHERE (deleted_at IS NULL);
-- 同一事件同一天同一種提醒不重發（run_check 冪等靠這個）。
CREATE UNIQUE INDEX IF NOT EXISTS energy_notification_event_kind_day
  ON public.tbl_energy_notification USING btree (event_id, kind, notify_on) WHERE (deleted_at IS NULL);
