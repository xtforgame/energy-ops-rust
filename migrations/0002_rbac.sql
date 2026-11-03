-- RBAC：permission（simple／complex）、角色、授權（grant）。照 az rbacHardening SPEC §2。

CREATE TABLE tbl_rbac_permission (
  id bigserial PRIMARY KEY,
  scope text NOT NULL CHECK (scope IN ('organization', 'project', 'resource')),
  type text NOT NULL CHECK (type IN ('system', 'plugin', 'user-defined', 'synthetic')),
  created_by_plugin text,
  unique_name text NOT NULL,
  name text NOT NULL,
  complex boolean NOT NULL DEFAULT false,
  -- 自訂（user-defined）列屬於哪個組織；NULL＝全域。
  owning_organization_id bigint REFERENCES tbl_organization(id),
  data jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at timestamptz
);
CREATE UNIQUE INDEX tbl_rbac_permission_uq ON tbl_rbac_permission (scope, unique_name) WHERE deleted_at IS NULL;
SELECT eo_attach_touch('tbl_rbac_permission');

-- complex → simple，只展開一層。
CREATE TABLE mn_rbac_complex_permission (
  id bigserial PRIMARY KEY,
  complex_one_id bigint NOT NULL REFERENCES tbl_rbac_permission(id),
  simple_one_id bigint NOT NULL REFERENCES tbl_rbac_permission(id),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (complex_one_id, simple_one_id)
);

CREATE TABLE tbl_rbac_role (
  id bigserial PRIMARY KEY,
  scope text NOT NULL CHECK (scope IN ('organization', 'project', 'resource')),
  type text NOT NULL CHECK (type IN ('system', 'plugin', 'user-defined', 'synthetic')),
  created_by_plugin text,
  unique_name text NOT NULL,
  name text NOT NULL,
  owning_organization_id bigint REFERENCES tbl_organization(id),
  data jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at timestamptz
);
CREATE UNIQUE INDEX tbl_rbac_role_uq ON tbl_rbac_role (scope, unique_name) WHERE deleted_at IS NULL;
SELECT eo_attach_touch('tbl_rbac_role');

CREATE TABLE mn_rbac_role_permission (
  id bigserial PRIMARY KEY,
  role_id bigint NOT NULL REFERENCES tbl_rbac_role(id),
  permission_id bigint NOT NULL REFERENCES tbl_rbac_permission(id),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (role_id, permission_id)
);

-- 授權。設了哪幾欄決定層級：只有 org＝組織層；有 project＝專案層；有 resource_identifier＝資源層。
-- 撤銷＝軟刪除並記 revoked_*。
CREATE TABLE tbl_user_rbac_role (
  id bigserial PRIMARY KEY,
  user_id bigint NOT NULL REFERENCES tbl_user(id),
  role_id bigint NOT NULL REFERENCES tbl_rbac_role(id),
  organization_id bigint REFERENCES tbl_organization(id),
  project_id bigint REFERENCES tbl_project(id),
  resource_identifier text NOT NULL DEFAULT '',
  added_by_user_id bigint REFERENCES tbl_user(id),
  revoked_by_user_id bigint REFERENCES tbl_user(id),
  revoked_at timestamptz,
  revoked_reason text,
  data jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at timestamptz
);
CREATE UNIQUE INDEX tbl_user_rbac_role_uq ON tbl_user_rbac_role (
  user_id, role_id, COALESCE(organization_id, 0), COALESCE(project_id, 0), resource_identifier
) WHERE deleted_at IS NULL;
CREATE INDEX tbl_user_rbac_role_user_idx ON tbl_user_rbac_role (user_id) WHERE deleted_at IS NULL;
CREATE INDEX tbl_user_rbac_role_org_idx ON tbl_user_rbac_role (organization_id) WHERE deleted_at IS NULL;
SELECT eo_attach_touch('tbl_user_rbac_role');
