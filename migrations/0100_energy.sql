-- 能源營運的資料表。由 App 5 `db/migrations/001_schema.sql`（85631c6）抽出能源表與其約束、索引，內容不改；
-- 參照的平台表（tbl_user、tbl_organization、tbl_project）與 0001_platform.sql 同名。
-- 不搬：RLS、energy_rbac_check（判定在 Rust）、pg_cron／pg_net（辨識對帳改成背景任務）、
-- Sequelize 自動建的 created_at／updated_at／deleted_at 單欄索引。

create table if not exists public.mn_energy_contract_station (
    id bigserial NOT NULL,
    valid_from date,
    valid_to date,
    params jsonb DEFAULT '{}'::jsonb NOT NULL,
    note text,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    station_id bigint,
    contract_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.mn_energy_department_member (
    id bigserial NOT NULL,
    role character varying(20) DEFAULT 'member'::character varying NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    user_id bigint NOT NULL,
    department_id bigint NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_attachment (
    id bigserial NOT NULL,
    storage_key character varying(512) NOT NULL,
    original_filename character varying(512) NOT NULL,
    mime_type character varying(128) NOT NULL,
    size bigint NOT NULL,
    hash character varying(128) NOT NULL,
    hash_algo character varying(32) DEFAULT 'sha256'::character varying NOT NULL,
    uploaded_at timestamp with time zone NOT NULL,
    status character varying(20) DEFAULT 'active'::character varying NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    execution_id bigint,
    ingest_batch_id bigint,
    uploaded_by_user_id bigint,
    supersedes_attachment_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone,
    CONSTRAINT energy_attachment_owner_chk CHECK (((execution_id IS NOT NULL) OR (ingest_batch_id IS NOT NULL)))
);

create table if not exists public.tbl_energy_attachment_link (
    id bigserial NOT NULL,
    role character varying(40) DEFAULT 'submitted'::character varying NOT NULL,
    seq integer DEFAULT 0 NOT NULL,
    note text,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    attachment_id bigint NOT NULL,
    task_id bigint,
    linked_by_user_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_audit_event (
    id bigserial NOT NULL,
    actor_kind character varying(20) DEFAULT 'user'::character varying NOT NULL,
    actor_label character varying(200),
    type character varying(120) NOT NULL,
    target_type character varying(60) NOT NULL,
    target_id character varying(120) NOT NULL,
    target_label character varying(300),
    outcome character varying(20) DEFAULT 'success'::character varying NOT NULL,
    summary text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    trace_id character varying(64),
    request_ip character varying(64),
    occurred_at timestamp with time zone NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    project_id bigint,
    actor_user_id bigint,
    on_behalf_of_user_id bigint,
    caused_by_event_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_bill (
    id bigserial NOT NULL,
    customer_number_raw character varying(60),
    ocr_raw jsonb DEFAULT '{}'::jsonb NOT NULL,
    ocr_extracted jsonb DEFAULT '{}'::jsonb NOT NULL,
    template_id character varying(40),
    doc_type character varying(24),
    due_date date,
    billing_period_start date,
    billing_period_end date,
    total_amount numeric(14,2),
    generation_kwh numeric(14,3),
    issued_at date,
    business_key character varying(200),
    document_no character varying(120),
    approval_state character varying(20) DEFAULT 'pending'::character varying NOT NULL,
    approved_at timestamp with time zone,
    review_status character varying(24) DEFAULT 'pendingReview'::character varying NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    station_id bigint,
    source_attachment_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_calendar_event (
    id bigserial NOT NULL,
    event_date date NOT NULL,
    kind character varying(40) NOT NULL,
    title character varying(300) NOT NULL,
    status character varying(24) DEFAULT 'pending'::character varying NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    landowner_id bigint,
    station_id bigint,
    contract_id bigint,
    installment_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_contract (
    id bigserial NOT NULL,
    contract_no character varying(80) NOT NULL,
    sign_method character varying(40),
    valid_from date NOT NULL,
    valid_to date,
    stage character varying(24) DEFAULT 'operating'::character varying NOT NULL,
    installment_total integer,
    note text,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    landowner_id bigint NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_department (
    id bigserial NOT NULL,
    key character varying(80) NOT NULL,
    name character varying(200) NOT NULL,
    sort_order integer DEFAULT 0 NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_execution_lineage (
    id bigserial NOT NULL,
    relation_kind character varying(40) DEFAULT 'derived'::character varying NOT NULL,
    note text,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    upstream_execution_id bigint NOT NULL,
    downstream_execution_id bigint NOT NULL,
    created_by_user_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_ingest_batch (
    id bigserial NOT NULL,
    submitted_at timestamp with time zone NOT NULL,
    file_count integer DEFAULT 0 NOT NULL,
    status character varying(24) DEFAULT 'uploading'::character varying NOT NULL,
    note character varying(300),
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    submitted_by_user_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_ingest_item (
    id bigserial NOT NULL,
    original_filename character varying(300),
    doc_type character varying(40),
    workflow_key character varying(60),
    classify_confidence numeric(5,4),
    status character varying(24) DEFAULT 'classified'::character varying NOT NULL,
    error_message text,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    batch_id bigint NOT NULL,
    attachment_id bigint,
    bill_id bigint,
    execution_id bigint,
    installment_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_installment (
    id bigserial NOT NULL,
    seq integer NOT NULL,
    due_date date NOT NULL,
    amount numeric(14,2) NOT NULL,
    business_key character varying(200),
    document_no character varying(120),
    approval_state character varying(20) DEFAULT 'pending'::character varying NOT NULL,
    approved_at timestamp with time zone,
    status character varying(24) DEFAULT 'pending'::character varying NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    contract_id bigint NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_landowner (
    id bigserial NOT NULL,
    name character varying(200) NOT NULL,
    tax_id character varying(40),
    contact_name character varying(200),
    contact_phone character varying(60),
    contact_email character varying(200),
    bank_name character varying(120),
    bank_branch character varying(120),
    bank_account character varying(60),
    bank_account_name character varying(200),
    note text,
    status character varying(20) DEFAULT 'active'::character varying NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_payout_record (
    id bigserial NOT NULL,
    strategy_type character varying(60) NOT NULL,
    params_snapshot jsonb DEFAULT '{}'::jsonb NOT NULL,
    input_snapshot jsonb DEFAULT '{}'::jsonb NOT NULL,
    amount numeric(14,2) NOT NULL,
    currency character varying(8) DEFAULT 'TWD'::character varying NOT NULL,
    business_date date NOT NULL,
    computed_at timestamp with time zone NOT NULL,
    trigger character varying(24) NOT NULL,
    superseded_at timestamp with time zone,
    status character varying(20) DEFAULT 'active'::character varying NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    station_id bigint NOT NULL,
    bill_id bigint NOT NULL,
    strategy_id bigint,
    computed_by_user_id bigint,
    execution_id bigint,
    supersedes_record_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_rating_strategy (
    id bigserial NOT NULL,
    key character varying(120) NOT NULL,
    version integer NOT NULL,
    name character varying(300) NOT NULL,
    type character varying(60) NOT NULL,
    params jsonb DEFAULT '{}'::jsonb NOT NULL,
    status character varying(20) DEFAULT 'draft'::character varying NOT NULL,
    published_at timestamp with time zone,
    currency character varying(8) DEFAULT 'TWD'::character varying NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    parent_version_id bigint,
    published_by_user_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_rating_strategy_binding (
    id bigserial NOT NULL,
    strategy_key character varying(120) NOT NULL,
    valid_from date NOT NULL,
    valid_to date,
    note text,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    station_id bigint NOT NULL,
    strategy_id bigint NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_station (
    id bigserial NOT NULL,
    customer_number character varying(60) NOT NULL,
    customer_number_key character varying(60),
    name character varying(200) NOT NULL,
    station_type character varying(20) DEFAULT 'solar'::character varying NOT NULL,
    latitude numeric(10,7),
    longitude numeric(10,7),
    address character varying(300),
    geocode_precision character varying(20),
    geocode_source character varying(40),
    geocoded_at timestamp with time zone,
    status character varying(24) DEFAULT 'operating'::character varying NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    landowner_id bigint NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_workflow_action (
    id bigserial NOT NULL,
    action_key character varying(120) NOT NULL,
    label_key character varying(200) NOT NULL,
    kind character varying(20) NOT NULL,
    target_mode character varying(24),
    target_node_key character varying(120),
    who jsonb DEFAULT '{}'::jsonb NOT NULL,
    requires_reason boolean DEFAULT false NOT NULL,
    locks_revert boolean DEFAULT false NOT NULL,
    side_effect_key character varying(80),
    state_key character varying(80),
    sort_order integer DEFAULT 0 NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    definition_id bigint NOT NULL,
    node_id bigint NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_workflow_binding (
    id bigserial NOT NULL,
    station_key character varying(120),
    doc_type character varying(24),
    workflow_key character varying(120) NOT NULL,
    valid_from date NOT NULL,
    valid_to date,
    note text,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    station_id bigint,
    contract_id bigint,
    definition_id bigint NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_workflow_definition (
    id bigserial NOT NULL,
    key character varying(120) NOT NULL,
    version integer NOT NULL,
    name character varying(300) NOT NULL,
    description text,
    status character varying(20) DEFAULT 'draft'::character varying NOT NULL,
    published_at timestamp with time zone,
    subject_type character varying(60) NOT NULL,
    abort_enabled boolean DEFAULT true NOT NULL,
    abort_approval_rule jsonb DEFAULT '{}'::jsonb NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    parent_version_id bigint,
    published_by_user_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_workflow_delegation (
    id bigserial NOT NULL,
    valid_from date NOT NULL,
    valid_to date,
    reason text,
    revoked_at timestamp with time zone,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    delegator_user_id bigint NOT NULL,
    delegate_user_id bigint NOT NULL,
    created_by_user_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_workflow_execution (
    id bigserial NOT NULL,
    workflow_key character varying(120),
    station_key character varying(120) NOT NULL,
    subject_type character varying(60) NOT NULL,
    subject_id character varying(120) NOT NULL,
    business_date date NOT NULL,
    status character varying(20) DEFAULT 'running'::character varying NOT NULL,
    current_node_key character varying(120),
    abort_reason text,
    started_at timestamp with time zone NOT NULL,
    ended_at timestamp with time zone,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    organization_id bigint NOT NULL,
    definition_id bigint NOT NULL,
    station_id bigint,
    abort_requested_by_user_id bigint,
    started_by_user_id bigint,
    supersedes_execution_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_workflow_node (
    id bigserial NOT NULL,
    node_key character varying(120) NOT NULL,
    name character varying(300) NOT NULL,
    allows_subject_edit boolean DEFAULT false NOT NULL,
    node_type character varying(40) NOT NULL,
    assignment_rule jsonb DEFAULT '{}'::jsonb NOT NULL,
    allow_self_approval boolean DEFAULT false NOT NULL,
    revertable boolean DEFAULT true NOT NULL,
    allow_jump_target boolean DEFAULT false NOT NULL,
    parallel boolean DEFAULT false NOT NULL,
    join_policy character varying(20),
    quorum_count integer,
    join_node_key character varying(120),
    service_key character varying(120),
    layout jsonb DEFAULT '{}'::jsonb NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    definition_id bigint NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_workflow_side_effect (
    id bigserial NOT NULL,
    side_effect_key character varying(80) NOT NULL,
    locks_revert boolean DEFAULT true NOT NULL,
    applied_at timestamp with time zone NOT NULL,
    note text,
    released_at timestamp with time zone,
    release_reason text,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    execution_id bigint NOT NULL,
    task_id bigint,
    applied_by_user_id bigint,
    released_by_user_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_workflow_task (
    id bigserial NOT NULL,
    node_key character varying(120) NOT NULL,
    node_type character varying(40) NOT NULL,
    seq integer NOT NULL,
    branch_key character varying(120),
    is_out_of_band boolean DEFAULT false NOT NULL,
    state character varying(20) DEFAULT 'ready'::character varying NOT NULL,
    outcome character varying(40),
    reason text,
    claimed_at timestamp with time zone,
    started_at timestamp with time zone,
    completed_at timestamp with time zone,
    due_at timestamp with time zone,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    execution_id bigint NOT NULL,
    assignee_user_id bigint,
    on_behalf_of_user_id bigint,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_workflow_task_candidate (
    id bigserial NOT NULL,
    kind character varying(24) DEFAULT 'potentialOwner'::character varying NOT NULL,
    resolved_by character varying(40) NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    task_id bigint NOT NULL,
    user_id bigint NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

create table if not exists public.tbl_energy_workflow_transition (
    id bigserial NOT NULL,
    from_node_key character varying(120) NOT NULL,
    to_node_key character varying(120) NOT NULL,
    kind character varying(20) DEFAULT 'forward'::character varying NOT NULL,
    label character varying(200),
    condition jsonb DEFAULT '{}'::jsonb NOT NULL,
    requires_reason boolean DEFAULT false NOT NULL,
    sort_order integer DEFAULT 0 NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    definition_id bigint NOT NULL,
    created_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    updated_at timestamp with time zone DEFAULT clock_timestamp() NOT NULL,
    deleted_at timestamp with time zone
);

alter table only public.mn_energy_contract_station add constraint mn_energy_contract_station_pkey PRIMARY KEY (id);

alter table only public.mn_energy_department_member add constraint mn_energy_department_member_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_attachment_link add constraint tbl_energy_attachment_link_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_attachment add constraint tbl_energy_attachment_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_audit_event add constraint tbl_energy_audit_event_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_bill add constraint tbl_energy_bill_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_calendar_event add constraint tbl_energy_calendar_event_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_contract add constraint tbl_energy_contract_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_department add constraint tbl_energy_department_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_execution_lineage add constraint tbl_energy_execution_lineage_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_ingest_batch add constraint tbl_energy_ingest_batch_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_ingest_item add constraint tbl_energy_ingest_item_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_installment add constraint tbl_energy_installment_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_landowner add constraint tbl_energy_landowner_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_payout_record add constraint tbl_energy_payout_record_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_rating_strategy_binding add constraint tbl_energy_rating_strategy_binding_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_rating_strategy add constraint tbl_energy_rating_strategy_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_station add constraint tbl_energy_station_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_workflow_action add constraint tbl_energy_workflow_action_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_workflow_binding add constraint tbl_energy_workflow_binding_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_workflow_definition add constraint tbl_energy_workflow_definition_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_workflow_delegation add constraint tbl_energy_workflow_delegation_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_workflow_execution add constraint tbl_energy_workflow_execution_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_workflow_node add constraint tbl_energy_workflow_node_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_workflow_side_effect add constraint tbl_energy_workflow_side_effect_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_workflow_task_candidate add constraint tbl_energy_workflow_task_candidate_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_workflow_task add constraint tbl_energy_workflow_task_pkey PRIMARY KEY (id);

alter table only public.tbl_energy_workflow_transition add constraint tbl_energy_workflow_transition_pkey PRIMARY KEY (id);

alter table only public.mn_energy_contract_station add constraint mn_energy_contract_station_contract_id_fkey FOREIGN KEY (contract_id) REFERENCES public.tbl_energy_contract(id) ON UPDATE CASCADE ON DELETE SET NULL;

alter table only public.mn_energy_contract_station add constraint mn_energy_contract_station_station_id_fkey FOREIGN KEY (station_id) REFERENCES public.tbl_energy_station(id) ON UPDATE CASCADE ON DELETE SET NULL;

alter table only public.mn_energy_department_member add constraint mn_energy_department_member_department_id_fkey FOREIGN KEY (department_id) REFERENCES public.tbl_energy_department(id) ON UPDATE CASCADE ON DELETE SET NULL;

alter table only public.mn_energy_department_member add constraint mn_energy_department_member_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE SET NULL;

alter table only public.tbl_energy_attachment add constraint tbl_energy_attachment_execution_id_fkey FOREIGN KEY (execution_id) REFERENCES public.tbl_energy_workflow_execution(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_attachment add constraint tbl_energy_attachment_ingest_batch_id_fkey FOREIGN KEY (ingest_batch_id) REFERENCES public.tbl_energy_ingest_batch(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_attachment_link add constraint tbl_energy_attachment_link_attachment_id_fkey FOREIGN KEY (attachment_id) REFERENCES public.tbl_energy_attachment(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_attachment_link add constraint tbl_energy_attachment_link_linked_by_user_id_fkey FOREIGN KEY (linked_by_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_attachment_link add constraint tbl_energy_attachment_link_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.tbl_energy_workflow_task(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_attachment add constraint tbl_energy_attachment_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_attachment add constraint tbl_energy_attachment_supersedes_attachment_id_fkey FOREIGN KEY (supersedes_attachment_id) REFERENCES public.tbl_energy_attachment(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_attachment add constraint tbl_energy_attachment_uploaded_by_user_id_fkey FOREIGN KEY (uploaded_by_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_audit_event add constraint tbl_energy_audit_event_actor_user_id_fkey FOREIGN KEY (actor_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_audit_event add constraint tbl_energy_audit_event_caused_by_event_id_fkey FOREIGN KEY (caused_by_event_id) REFERENCES public.tbl_energy_audit_event(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_audit_event add constraint tbl_energy_audit_event_on_behalf_of_user_id_fkey FOREIGN KEY (on_behalf_of_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_audit_event add constraint tbl_energy_audit_event_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_audit_event add constraint tbl_energy_audit_event_project_id_fkey FOREIGN KEY (project_id) REFERENCES public.tbl_project(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_bill add constraint tbl_energy_bill_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_bill add constraint tbl_energy_bill_source_attachment_id_fkey FOREIGN KEY (source_attachment_id) REFERENCES public.tbl_energy_attachment(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_bill add constraint tbl_energy_bill_station_id_fkey FOREIGN KEY (station_id) REFERENCES public.tbl_energy_station(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_calendar_event add constraint tbl_energy_calendar_event_contract_id_fkey FOREIGN KEY (contract_id) REFERENCES public.tbl_energy_contract(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_calendar_event add constraint tbl_energy_calendar_event_installment_id_fkey FOREIGN KEY (installment_id) REFERENCES public.tbl_energy_installment(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_calendar_event add constraint tbl_energy_calendar_event_landowner_id_fkey FOREIGN KEY (landowner_id) REFERENCES public.tbl_energy_landowner(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_calendar_event add constraint tbl_energy_calendar_event_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_calendar_event add constraint tbl_energy_calendar_event_station_id_fkey FOREIGN KEY (station_id) REFERENCES public.tbl_energy_station(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_contract add constraint tbl_energy_contract_landowner_id_fkey FOREIGN KEY (landowner_id) REFERENCES public.tbl_energy_landowner(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_contract add constraint tbl_energy_contract_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_department add constraint tbl_energy_department_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_execution_lineage add constraint tbl_energy_execution_lineage_created_by_user_id_fkey FOREIGN KEY (created_by_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_execution_lineage add constraint tbl_energy_execution_lineage_downstream_execution_id_fkey FOREIGN KEY (downstream_execution_id) REFERENCES public.tbl_energy_workflow_execution(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_execution_lineage add constraint tbl_energy_execution_lineage_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_execution_lineage add constraint tbl_energy_execution_lineage_upstream_execution_id_fkey FOREIGN KEY (upstream_execution_id) REFERENCES public.tbl_energy_workflow_execution(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_ingest_batch add constraint tbl_energy_ingest_batch_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_ingest_batch add constraint tbl_energy_ingest_batch_submitted_by_user_id_fkey FOREIGN KEY (submitted_by_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_ingest_item add constraint tbl_energy_ingest_item_attachment_id_fkey FOREIGN KEY (attachment_id) REFERENCES public.tbl_energy_attachment(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_ingest_item add constraint tbl_energy_ingest_item_batch_id_fkey FOREIGN KEY (batch_id) REFERENCES public.tbl_energy_ingest_batch(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_ingest_item add constraint tbl_energy_ingest_item_bill_id_fkey FOREIGN KEY (bill_id) REFERENCES public.tbl_energy_bill(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_ingest_item add constraint tbl_energy_ingest_item_execution_id_fkey FOREIGN KEY (execution_id) REFERENCES public.tbl_energy_workflow_execution(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_ingest_item add constraint tbl_energy_ingest_item_installment_id_fkey FOREIGN KEY (installment_id) REFERENCES public.tbl_energy_installment(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_ingest_item add constraint tbl_energy_ingest_item_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_installment add constraint tbl_energy_installment_contract_id_fkey FOREIGN KEY (contract_id) REFERENCES public.tbl_energy_contract(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_installment add constraint tbl_energy_installment_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_landowner add constraint tbl_energy_landowner_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_payout_record add constraint tbl_energy_payout_record_bill_id_fkey FOREIGN KEY (bill_id) REFERENCES public.tbl_energy_bill(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_payout_record add constraint tbl_energy_payout_record_computed_by_user_id_fkey FOREIGN KEY (computed_by_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_payout_record add constraint tbl_energy_payout_record_execution_id_fkey FOREIGN KEY (execution_id) REFERENCES public.tbl_energy_workflow_execution(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_payout_record add constraint tbl_energy_payout_record_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_payout_record add constraint tbl_energy_payout_record_station_id_fkey FOREIGN KEY (station_id) REFERENCES public.tbl_energy_station(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_payout_record add constraint tbl_energy_payout_record_strategy_id_fkey FOREIGN KEY (strategy_id) REFERENCES public.tbl_energy_rating_strategy(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_payout_record add constraint tbl_energy_payout_record_supersedes_record_id_fkey FOREIGN KEY (supersedes_record_id) REFERENCES public.tbl_energy_payout_record(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_rating_strategy_binding add constraint tbl_energy_rating_strategy_binding_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_rating_strategy_binding add constraint tbl_energy_rating_strategy_binding_station_id_fkey FOREIGN KEY (station_id) REFERENCES public.tbl_energy_station(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_rating_strategy_binding add constraint tbl_energy_rating_strategy_binding_strategy_id_fkey FOREIGN KEY (strategy_id) REFERENCES public.tbl_energy_rating_strategy(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_rating_strategy add constraint tbl_energy_rating_strategy_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_rating_strategy add constraint tbl_energy_rating_strategy_parent_version_id_fkey FOREIGN KEY (parent_version_id) REFERENCES public.tbl_energy_rating_strategy(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_rating_strategy add constraint tbl_energy_rating_strategy_published_by_user_id_fkey FOREIGN KEY (published_by_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_station add constraint tbl_energy_station_landowner_id_fkey FOREIGN KEY (landowner_id) REFERENCES public.tbl_energy_landowner(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_station add constraint tbl_energy_station_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_action add constraint tbl_energy_workflow_action_definition_id_fkey FOREIGN KEY (definition_id) REFERENCES public.tbl_energy_workflow_definition(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_action add constraint tbl_energy_workflow_action_node_id_fkey FOREIGN KEY (node_id) REFERENCES public.tbl_energy_workflow_node(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_binding add constraint tbl_energy_workflow_binding_contract_id_fkey FOREIGN KEY (contract_id) REFERENCES public.tbl_energy_contract(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_binding add constraint tbl_energy_workflow_binding_definition_id_fkey FOREIGN KEY (definition_id) REFERENCES public.tbl_energy_workflow_definition(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_binding add constraint tbl_energy_workflow_binding_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_binding add constraint tbl_energy_workflow_binding_station_id_fkey FOREIGN KEY (station_id) REFERENCES public.tbl_energy_station(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_definition add constraint tbl_energy_workflow_definition_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_definition add constraint tbl_energy_workflow_definition_parent_version_id_fkey FOREIGN KEY (parent_version_id) REFERENCES public.tbl_energy_workflow_definition(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_definition add constraint tbl_energy_workflow_definition_published_by_user_id_fkey FOREIGN KEY (published_by_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_delegation add constraint tbl_energy_workflow_delegation_created_by_user_id_fkey FOREIGN KEY (created_by_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_delegation add constraint tbl_energy_workflow_delegation_delegate_user_id_fkey FOREIGN KEY (delegate_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_delegation add constraint tbl_energy_workflow_delegation_delegator_user_id_fkey FOREIGN KEY (delegator_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_delegation add constraint tbl_energy_workflow_delegation_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_execution add constraint tbl_energy_workflow_execution_abort_requested_by_user_id_fkey FOREIGN KEY (abort_requested_by_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_execution add constraint tbl_energy_workflow_execution_definition_id_fkey FOREIGN KEY (definition_id) REFERENCES public.tbl_energy_workflow_definition(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_execution add constraint tbl_energy_workflow_execution_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.tbl_organization(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_execution add constraint tbl_energy_workflow_execution_started_by_user_id_fkey FOREIGN KEY (started_by_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_execution add constraint tbl_energy_workflow_execution_station_id_fkey FOREIGN KEY (station_id) REFERENCES public.tbl_energy_station(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_execution add constraint tbl_energy_workflow_execution_supersedes_execution_id_fkey FOREIGN KEY (supersedes_execution_id) REFERENCES public.tbl_energy_workflow_execution(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_node add constraint tbl_energy_workflow_node_definition_id_fkey FOREIGN KEY (definition_id) REFERENCES public.tbl_energy_workflow_definition(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_side_effect add constraint tbl_energy_workflow_side_effect_applied_by_user_id_fkey FOREIGN KEY (applied_by_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_side_effect add constraint tbl_energy_workflow_side_effect_execution_id_fkey FOREIGN KEY (execution_id) REFERENCES public.tbl_energy_workflow_execution(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_side_effect add constraint tbl_energy_workflow_side_effect_released_by_user_id_fkey FOREIGN KEY (released_by_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_side_effect add constraint tbl_energy_workflow_side_effect_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.tbl_energy_workflow_task(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_task add constraint tbl_energy_workflow_task_assignee_user_id_fkey FOREIGN KEY (assignee_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_task_candidate add constraint tbl_energy_workflow_task_candidate_task_id_fkey FOREIGN KEY (task_id) REFERENCES public.tbl_energy_workflow_task(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_task_candidate add constraint tbl_energy_workflow_task_candidate_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_task add constraint tbl_energy_workflow_task_execution_id_fkey FOREIGN KEY (execution_id) REFERENCES public.tbl_energy_workflow_execution(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_task add constraint tbl_energy_workflow_task_on_behalf_of_user_id_fkey FOREIGN KEY (on_behalf_of_user_id) REFERENCES public.tbl_user(id) ON UPDATE CASCADE ON DELETE CASCADE;

alter table only public.tbl_energy_workflow_transition add constraint tbl_energy_workflow_transition_definition_id_fkey FOREIGN KEY (definition_id) REFERENCES public.tbl_energy_workflow_definition(id) ON UPDATE CASCADE ON DELETE CASCADE;

create index if not exists energy_attachment_execution ON public.tbl_energy_attachment USING btree (execution_id, uploaded_at);

create index if not exists energy_attachment_link_attach ON public.tbl_energy_attachment_link USING btree (attachment_id);

create index if not exists energy_attachment_link_task ON public.tbl_energy_attachment_link USING btree (task_id, role, seq);

create unique index if not exists energy_attachment_link_unique ON public.tbl_energy_attachment_link USING btree (attachment_id, task_id, role) WHERE (deleted_at IS NULL);

create index if not exists energy_attachment_org_hash ON public.tbl_energy_attachment USING btree (organization_id, hash, hash_algo);

create unique index if not exists energy_attachment_storage_key ON public.tbl_energy_attachment USING btree (storage_key) WHERE (deleted_at IS NULL);

create index if not exists energy_attachment_supersedes ON public.tbl_energy_attachment USING btree (supersedes_attachment_id);

create index if not exists energy_audit_event_caused_by ON public.tbl_energy_audit_event USING btree (caused_by_event_id);

create index if not exists energy_audit_event_org_actor ON public.tbl_energy_audit_event USING btree (organization_id, actor_user_id, occurred_at);

create index if not exists energy_audit_event_org_on_behalf ON public.tbl_energy_audit_event USING btree (organization_id, on_behalf_of_user_id, occurred_at);

create index if not exists energy_audit_event_org_target ON public.tbl_energy_audit_event USING btree (organization_id, target_type, target_id, occurred_at);

create index if not exists energy_audit_event_org_time ON public.tbl_energy_audit_event USING btree (organization_id, occurred_at);

create index if not exists energy_audit_event_org_type ON public.tbl_energy_audit_event USING btree (organization_id, type, occurred_at);

create index if not exists energy_audit_event_trace ON public.tbl_energy_audit_event USING btree (trace_id);

create unique index if not exists energy_bill_org_business_key ON public.tbl_energy_bill USING btree (organization_id, business_key) WHERE (((approval_state)::text = 'approved'::text) AND (deleted_at IS NULL));

create index if not exists energy_bill_org_customer_raw ON public.tbl_energy_bill USING btree (organization_id, customer_number_raw);

create unique index if not exists energy_bill_org_document_no ON public.tbl_energy_bill USING btree (organization_id, document_no) WHERE (((approval_state)::text = 'approved'::text) AND (deleted_at IS NULL));

create index if not exists energy_bill_org_period ON public.tbl_energy_bill USING btree (organization_id, billing_period_end);

create index if not exists energy_bill_org_review_status ON public.tbl_energy_bill USING btree (organization_id, review_status);

create index if not exists energy_bill_org_station_period ON public.tbl_energy_bill USING btree (organization_id, station_id, billing_period_end);

create index if not exists energy_calendar_event_org_date ON public.tbl_energy_calendar_event USING btree (organization_id, event_date, status);

create index if not exists energy_calendar_event_org_kind ON public.tbl_energy_calendar_event USING btree (organization_id, kind, event_date);

create unique index if not exists energy_contract_no ON public.tbl_energy_contract USING btree (organization_id, contract_no) WHERE (deleted_at IS NULL);

create index if not exists energy_contract_org_landowner ON public.tbl_energy_contract USING btree (organization_id, landowner_id);

create unique index if not exists energy_contract_station_current ON public.mn_energy_contract_station USING btree (contract_id, station_id, valid_from) WHERE (deleted_at IS NULL);

create index if not exists energy_contract_station_station ON public.mn_energy_contract_station USING btree (station_id);

create index if not exists energy_department_member_role ON public.mn_energy_department_member USING btree (department_id, role);

create unique index if not exists energy_department_member_unique ON public.mn_energy_department_member USING btree (department_id, user_id) WHERE (deleted_at IS NULL);

create index if not exists energy_department_member_user ON public.mn_energy_department_member USING btree (user_id);

create unique index if not exists energy_department_org_key ON public.tbl_energy_department USING btree (organization_id, key) WHERE (deleted_at IS NULL);

create index if not exists energy_execution_lineage_down ON public.tbl_energy_execution_lineage USING btree (downstream_execution_id, relation_kind);

create index if not exists energy_execution_lineage_org ON public.tbl_energy_execution_lineage USING btree (organization_id);

create unique index if not exists energy_execution_lineage_uniq ON public.tbl_energy_execution_lineage USING btree (upstream_execution_id, downstream_execution_id, relation_kind) WHERE (deleted_at IS NULL);

create index if not exists energy_execution_lineage_up ON public.tbl_energy_execution_lineage USING btree (upstream_execution_id, relation_kind);

create index if not exists energy_ingest_batch_org_time ON public.tbl_energy_ingest_batch USING btree (organization_id, submitted_at);

create index if not exists energy_ingest_batch_org_user ON public.tbl_energy_ingest_batch USING btree (organization_id, submitted_by_user_id, submitted_at);

create index if not exists energy_ingest_item_batch ON public.tbl_energy_ingest_item USING btree (batch_id, id);

create index if not exists energy_ingest_item_bill ON public.tbl_energy_ingest_item USING btree (bill_id);

create index if not exists energy_ingest_item_org_status ON public.tbl_energy_ingest_item USING btree (organization_id, status);

create unique index if not exists energy_installment_contract_seq ON public.tbl_energy_installment USING btree (contract_id, seq) WHERE (deleted_at IS NULL);

create unique index if not exists energy_installment_org_business_key ON public.tbl_energy_installment USING btree (organization_id, business_key) WHERE (((approval_state)::text = 'approved'::text) AND (deleted_at IS NULL));

create unique index if not exists energy_installment_org_document_no ON public.tbl_energy_installment USING btree (organization_id, document_no) WHERE (deleted_at IS NULL);

create index if not exists energy_installment_org_due ON public.tbl_energy_installment USING btree (organization_id, due_date, status);

create index if not exists energy_landowner_org_name ON public.tbl_energy_landowner USING btree (organization_id, name);

create index if not exists energy_landowner_org_status ON public.tbl_energy_landowner USING btree (organization_id, status);

create index if not exists energy_payout_record_bill_all ON public.tbl_energy_payout_record USING btree (bill_id, computed_at);

create index if not exists energy_payout_record_execution ON public.tbl_energy_payout_record USING btree (execution_id);

create index if not exists energy_payout_record_org_bill_status ON public.tbl_energy_payout_record USING btree (organization_id, bill_id, status);

create index if not exists energy_payout_record_org_station_time ON public.tbl_energy_payout_record USING btree (organization_id, station_id, business_date);

create index if not exists energy_payout_record_supersedes ON public.tbl_energy_payout_record USING btree (supersedes_record_id);

create unique index if not exists energy_rating_strategy_binding_current ON public.tbl_energy_rating_strategy_binding USING btree (station_id, strategy_key) WHERE ((valid_to IS NULL) AND (deleted_at IS NULL));

create index if not exists energy_rating_strategy_binding_station_key ON public.tbl_energy_rating_strategy_binding USING btree (station_id, strategy_key, valid_from);

create index if not exists energy_rating_strategy_binding_strategy ON public.tbl_energy_rating_strategy_binding USING btree (strategy_id);

create index if not exists energy_rating_strategy_org_key_status ON public.tbl_energy_rating_strategy USING btree (organization_id, key, status);

create unique index if not exists energy_rating_strategy_org_key_version ON public.tbl_energy_rating_strategy USING btree (organization_id, key, version) WHERE (deleted_at IS NULL);

create index if not exists energy_rating_strategy_parent ON public.tbl_energy_rating_strategy USING btree (parent_version_id);

create unique index if not exists energy_station_org_customer_key ON public.tbl_energy_station USING btree (organization_id, customer_number_key) WHERE (deleted_at IS NULL);

create unique index if not exists energy_station_org_customer_number ON public.tbl_energy_station USING btree (organization_id, customer_number) WHERE (deleted_at IS NULL);

create index if not exists energy_station_org_landowner ON public.tbl_energy_station USING btree (organization_id, landowner_id);

create index if not exists energy_station_org_status ON public.tbl_energy_station USING btree (organization_id, status);

create index if not exists energy_workflow_action_definition ON public.tbl_energy_workflow_action USING btree (definition_id, kind);

create index if not exists energy_workflow_action_node ON public.tbl_energy_workflow_action USING btree (node_id, sort_order);

create unique index if not exists energy_workflow_action_node_key ON public.tbl_energy_workflow_action USING btree (node_id, action_key) WHERE (deleted_at IS NULL);

create index if not exists energy_workflow_binding_contract ON public.tbl_energy_workflow_binding USING btree (contract_id);

create unique index if not exists energy_workflow_binding_contract_current ON public.tbl_energy_workflow_binding USING btree (organization_id, contract_id, doc_type, workflow_key) WHERE ((valid_to IS NULL) AND (deleted_at IS NULL));

create unique index if not exists energy_workflow_binding_current ON public.tbl_energy_workflow_binding USING btree (organization_id, station_key, doc_type, workflow_key) WHERE ((valid_to IS NULL) AND (deleted_at IS NULL));

create index if not exists energy_workflow_binding_definition ON public.tbl_energy_workflow_binding USING btree (definition_id);

create index if not exists energy_workflow_binding_station_key_from ON public.tbl_energy_workflow_binding USING btree (organization_id, station_key, workflow_key, valid_from);

create index if not exists energy_workflow_definition_org_key_status ON public.tbl_energy_workflow_definition USING btree (organization_id, key, status);

create unique index if not exists energy_workflow_definition_org_key_version ON public.tbl_energy_workflow_definition USING btree (organization_id, key, version) WHERE (deleted_at IS NULL);

create index if not exists energy_workflow_definition_parent ON public.tbl_energy_workflow_definition USING btree (parent_version_id);

create index if not exists energy_workflow_delegation_org_delegate ON public.tbl_energy_workflow_delegation USING btree (organization_id, delegate_user_id, valid_from);

create index if not exists energy_workflow_delegation_org_delegator ON public.tbl_energy_workflow_delegation USING btree (organization_id, delegator_user_id, valid_from);

create index if not exists energy_workflow_execution_definition ON public.tbl_energy_workflow_execution USING btree (definition_id);

create unique index if not exists energy_workflow_execution_one_running ON public.tbl_energy_workflow_execution USING btree (organization_id, subject_type, subject_id, workflow_key) WHERE (((status)::text = 'running'::text) AND (deleted_at IS NULL));

create index if not exists energy_workflow_execution_org_status ON public.tbl_energy_workflow_execution USING btree (organization_id, status, started_at);

create index if not exists energy_workflow_execution_org_subject ON public.tbl_energy_workflow_execution USING btree (organization_id, subject_type, subject_id);

create index if not exists energy_workflow_execution_station ON public.tbl_energy_workflow_execution USING btree (station_key, started_at);

create index if not exists energy_workflow_execution_station_fk ON public.tbl_energy_workflow_execution USING btree (station_id);

create index if not exists energy_workflow_node_definition ON public.tbl_energy_workflow_node USING btree (definition_id);

create unique index if not exists energy_workflow_node_definition_key ON public.tbl_energy_workflow_node USING btree (definition_id, node_key) WHERE (deleted_at IS NULL);

create index if not exists energy_workflow_side_effect_execution ON public.tbl_energy_workflow_side_effect USING btree (execution_id);

create index if not exists energy_workflow_side_effect_key_time ON public.tbl_energy_workflow_side_effect USING btree (side_effect_key, applied_at);

create index if not exists energy_workflow_side_effect_locks ON public.tbl_energy_workflow_side_effect USING btree (execution_id) WHERE ((locks_revert = true) AND (released_at IS NULL) AND (deleted_at IS NULL));

create index if not exists energy_workflow_task_assignee_state ON public.tbl_energy_workflow_task USING btree (assignee_user_id, state);

create index if not exists energy_workflow_task_candidate_task_kind ON public.tbl_energy_workflow_task_candidate USING btree (task_id, kind);

create unique index if not exists energy_workflow_task_candidate_unique ON public.tbl_energy_workflow_task_candidate USING btree (task_id, user_id, kind) WHERE (deleted_at IS NULL);

create index if not exists energy_workflow_task_candidate_user ON public.tbl_energy_workflow_task_candidate USING btree (user_id, kind);

create unique index if not exists energy_workflow_task_execution_seq ON public.tbl_energy_workflow_task USING btree (execution_id, seq) WHERE (deleted_at IS NULL);

create index if not exists energy_workflow_task_execution_state ON public.tbl_energy_workflow_task USING btree (execution_id, state);

create index if not exists energy_workflow_task_node ON public.tbl_energy_workflow_task USING btree (execution_id, node_key, seq);

create index if not exists energy_workflow_transition_definition ON public.tbl_energy_workflow_transition USING btree (definition_id);

create index if not exists energy_workflow_transition_definition_from ON public.tbl_energy_workflow_transition USING btree (definition_id, from_node_key, sort_order);

create index if not exists mn_energy_contract_station_contract_id ON public.mn_energy_contract_station USING btree (contract_id);

create index if not exists mn_energy_department_member_department_id ON public.mn_energy_department_member USING btree (department_id);

create index if not exists tbl_energy_attachment_execution_id ON public.tbl_energy_attachment USING btree (execution_id);

create index if not exists tbl_energy_attachment_ingest_batch_id ON public.tbl_energy_attachment USING btree (ingest_batch_id);

create index if not exists tbl_energy_attachment_link_linked_by_user_id ON public.tbl_energy_attachment_link USING btree (linked_by_user_id);

create index if not exists tbl_energy_attachment_link_task_id ON public.tbl_energy_attachment_link USING btree (task_id);

create index if not exists tbl_energy_attachment_organization_id ON public.tbl_energy_attachment USING btree (organization_id);

create index if not exists tbl_energy_attachment_uploaded_by_user_id ON public.tbl_energy_attachment USING btree (uploaded_by_user_id);

create index if not exists tbl_energy_audit_event_actor_user_id ON public.tbl_energy_audit_event USING btree (actor_user_id);

create index if not exists tbl_energy_audit_event_on_behalf_of_user_id ON public.tbl_energy_audit_event USING btree (on_behalf_of_user_id);

create index if not exists tbl_energy_audit_event_organization_id ON public.tbl_energy_audit_event USING btree (organization_id);

create index if not exists tbl_energy_audit_event_project_id ON public.tbl_energy_audit_event USING btree (project_id);

create index if not exists tbl_energy_bill_organization_id ON public.tbl_energy_bill USING btree (organization_id);

create index if not exists tbl_energy_bill_source_attachment_id ON public.tbl_energy_bill USING btree (source_attachment_id);

create index if not exists tbl_energy_bill_station_id ON public.tbl_energy_bill USING btree (station_id);

create index if not exists tbl_energy_calendar_event_contract_id ON public.tbl_energy_calendar_event USING btree (contract_id);

create index if not exists tbl_energy_calendar_event_installment_id ON public.tbl_energy_calendar_event USING btree (installment_id);

create index if not exists tbl_energy_calendar_event_landowner_id ON public.tbl_energy_calendar_event USING btree (landowner_id);

create index if not exists tbl_energy_calendar_event_organization_id ON public.tbl_energy_calendar_event USING btree (organization_id);

create index if not exists tbl_energy_calendar_event_station_id ON public.tbl_energy_calendar_event USING btree (station_id);

create index if not exists tbl_energy_contract_landowner_id ON public.tbl_energy_contract USING btree (landowner_id);

create index if not exists tbl_energy_contract_organization_id ON public.tbl_energy_contract USING btree (organization_id);

create index if not exists tbl_energy_department_organization_id ON public.tbl_energy_department USING btree (organization_id);

create index if not exists tbl_energy_execution_lineage_created_by_user_id ON public.tbl_energy_execution_lineage USING btree (created_by_user_id);

create index if not exists tbl_energy_execution_lineage_downstream_execution_id ON public.tbl_energy_execution_lineage USING btree (downstream_execution_id);

create index if not exists tbl_energy_execution_lineage_upstream_execution_id ON public.tbl_energy_execution_lineage USING btree (upstream_execution_id);

create index if not exists tbl_energy_ingest_batch_organization_id ON public.tbl_energy_ingest_batch USING btree (organization_id);

create index if not exists tbl_energy_ingest_batch_submitted_by_user_id ON public.tbl_energy_ingest_batch USING btree (submitted_by_user_id);

create index if not exists tbl_energy_ingest_item_attachment_id ON public.tbl_energy_ingest_item USING btree (attachment_id);

create index if not exists tbl_energy_ingest_item_batch_id ON public.tbl_energy_ingest_item USING btree (batch_id);

create index if not exists tbl_energy_ingest_item_execution_id ON public.tbl_energy_ingest_item USING btree (execution_id);

create index if not exists tbl_energy_ingest_item_installment_id ON public.tbl_energy_ingest_item USING btree (installment_id);

create index if not exists tbl_energy_ingest_item_organization_id ON public.tbl_energy_ingest_item USING btree (organization_id);

create index if not exists tbl_energy_installment_contract_id ON public.tbl_energy_installment USING btree (contract_id);

create index if not exists tbl_energy_installment_organization_id ON public.tbl_energy_installment USING btree (organization_id);

create index if not exists tbl_energy_landowner_organization_id ON public.tbl_energy_landowner USING btree (organization_id);

create index if not exists tbl_energy_payout_record_bill_id ON public.tbl_energy_payout_record USING btree (bill_id);

create index if not exists tbl_energy_payout_record_computed_by_user_id ON public.tbl_energy_payout_record USING btree (computed_by_user_id);

create index if not exists tbl_energy_payout_record_organization_id ON public.tbl_energy_payout_record USING btree (organization_id);

create index if not exists tbl_energy_payout_record_station_id ON public.tbl_energy_payout_record USING btree (station_id);

create index if not exists tbl_energy_payout_record_strategy_id ON public.tbl_energy_payout_record USING btree (strategy_id);

create index if not exists tbl_energy_rating_strategy_binding_organization_id ON public.tbl_energy_rating_strategy_binding USING btree (organization_id);

create index if not exists tbl_energy_rating_strategy_binding_station_id ON public.tbl_energy_rating_strategy_binding USING btree (station_id);

create index if not exists tbl_energy_rating_strategy_organization_id ON public.tbl_energy_rating_strategy USING btree (organization_id);

create index if not exists tbl_energy_rating_strategy_published_by_user_id ON public.tbl_energy_rating_strategy USING btree (published_by_user_id);

create index if not exists tbl_energy_station_landowner_id ON public.tbl_energy_station USING btree (landowner_id);

create index if not exists tbl_energy_station_organization_id ON public.tbl_energy_station USING btree (organization_id);

create index if not exists tbl_energy_workflow_action_definition_id ON public.tbl_energy_workflow_action USING btree (definition_id);

create index if not exists tbl_energy_workflow_action_node_id ON public.tbl_energy_workflow_action USING btree (node_id);

create index if not exists tbl_energy_workflow_binding_organization_id ON public.tbl_energy_workflow_binding USING btree (organization_id);

create index if not exists tbl_energy_workflow_binding_station_id ON public.tbl_energy_workflow_binding USING btree (station_id);

create index if not exists tbl_energy_workflow_definition_organization_id ON public.tbl_energy_workflow_definition USING btree (organization_id);

create index if not exists tbl_energy_workflow_definition_published_by_user_id ON public.tbl_energy_workflow_definition USING btree (published_by_user_id);

create index if not exists tbl_energy_workflow_delegation_created_by_user_id ON public.tbl_energy_workflow_delegation USING btree (created_by_user_id);

create index if not exists tbl_energy_workflow_delegation_delegate_user_id ON public.tbl_energy_workflow_delegation USING btree (delegate_user_id);

create index if not exists tbl_energy_workflow_delegation_delegator_user_id ON public.tbl_energy_workflow_delegation USING btree (delegator_user_id);

create index if not exists tbl_energy_workflow_delegation_organization_id ON public.tbl_energy_workflow_delegation USING btree (organization_id);

create index if not exists tbl_energy_workflow_execution_abort_requested_by_user_id ON public.tbl_energy_workflow_execution USING btree (abort_requested_by_user_id);

create index if not exists tbl_energy_workflow_execution_organization_id ON public.tbl_energy_workflow_execution USING btree (organization_id);

create index if not exists tbl_energy_workflow_execution_started_by_user_id ON public.tbl_energy_workflow_execution USING btree (started_by_user_id);

create index if not exists tbl_energy_workflow_execution_supersedes_execution_id ON public.tbl_energy_workflow_execution USING btree (supersedes_execution_id);

create index if not exists tbl_energy_workflow_side_effect_applied_by_user_id ON public.tbl_energy_workflow_side_effect USING btree (applied_by_user_id);

create index if not exists tbl_energy_workflow_side_effect_released_by_user_id ON public.tbl_energy_workflow_side_effect USING btree (released_by_user_id);

create index if not exists tbl_energy_workflow_side_effect_task_id ON public.tbl_energy_workflow_side_effect USING btree (task_id);

create index if not exists tbl_energy_workflow_task_assignee_user_id ON public.tbl_energy_workflow_task USING btree (assignee_user_id);

create index if not exists tbl_energy_workflow_task_candidate_task_id ON public.tbl_energy_workflow_task_candidate USING btree (task_id);

create index if not exists tbl_energy_workflow_task_candidate_user_id ON public.tbl_energy_workflow_task_candidate USING btree (user_id);

create index if not exists tbl_energy_workflow_task_execution_id ON public.tbl_energy_workflow_task USING btree (execution_id);

create index if not exists tbl_energy_workflow_task_on_behalf_of_user_id ON public.tbl_energy_workflow_task USING btree (on_behalf_of_user_id);

-- updated_at 只在內容真的變了才更新。
SELECT eo_attach_touch('mn_energy_contract_station');
SELECT eo_attach_touch('mn_energy_department_member');
SELECT eo_attach_touch('tbl_energy_attachment');
SELECT eo_attach_touch('tbl_energy_attachment_link');
SELECT eo_attach_touch('tbl_energy_audit_event');
SELECT eo_attach_touch('tbl_energy_bill');
SELECT eo_attach_touch('tbl_energy_calendar_event');
SELECT eo_attach_touch('tbl_energy_contract');
SELECT eo_attach_touch('tbl_energy_department');
SELECT eo_attach_touch('tbl_energy_execution_lineage');
SELECT eo_attach_touch('tbl_energy_ingest_batch');
SELECT eo_attach_touch('tbl_energy_ingest_item');
SELECT eo_attach_touch('tbl_energy_installment');
SELECT eo_attach_touch('tbl_energy_landowner');
SELECT eo_attach_touch('tbl_energy_payout_record');
SELECT eo_attach_touch('tbl_energy_rating_strategy');
SELECT eo_attach_touch('tbl_energy_rating_strategy_binding');
SELECT eo_attach_touch('tbl_energy_station');
SELECT eo_attach_touch('tbl_energy_workflow_action');
SELECT eo_attach_touch('tbl_energy_workflow_binding');
SELECT eo_attach_touch('tbl_energy_workflow_definition');
SELECT eo_attach_touch('tbl_energy_workflow_delegation');
SELECT eo_attach_touch('tbl_energy_workflow_execution');
SELECT eo_attach_touch('tbl_energy_workflow_node');
SELECT eo_attach_touch('tbl_energy_workflow_side_effect');
SELECT eo_attach_touch('tbl_energy_workflow_task');
SELECT eo_attach_touch('tbl_energy_workflow_task_candidate');
SELECT eo_attach_touch('tbl_energy_workflow_transition');
