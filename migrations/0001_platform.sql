-- 平台：使用者、帳號、session、組織、專案、成員、邀請。
-- 共通欄位：id bigserial、data jsonb、created_at、updated_at、deleted_at（軟刪除）。

-- updated_at 只在內容真的變了才更新；單純設 deleted_at 不算變更。
CREATE OR REPLACE FUNCTION eo_touch_updated_at() RETURNS trigger AS $$
BEGIN
  IF (to_jsonb(NEW) - 'updated_at' - 'deleted_at') IS DISTINCT FROM (to_jsonb(OLD) - 'updated_at' - 'deleted_at') THEN
    NEW.updated_at := now();
  ELSE
    NEW.updated_at := OLD.updated_at;
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- 幫一張表掛上 trigger。
CREATE OR REPLACE FUNCTION eo_attach_touch(tbl regclass) RETURNS void AS $$
BEGIN
  EXECUTE format('CREATE TRIGGER %I BEFORE UPDATE ON %s FOR EACH ROW EXECUTE FUNCTION eo_touch_updated_at()',
                 'touch_' || replace(tbl::text, '.', '_'), tbl);
END;
$$ LANGUAGE plpgsql;

CREATE TABLE tbl_user (
  id bigserial PRIMARY KEY,
  name text NOT NULL,
  email text,
  -- 'admin'／'manager' 是超級使用者（RBAC 直接放行）；其他值或 NULL 是一般使用者。
  privilege text,
  disabled boolean NOT NULL DEFAULT false,
  picture text,
  data jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at timestamptz
);
CREATE UNIQUE INDEX tbl_user_email_uq ON tbl_user (lower(email)) WHERE deleted_at IS NULL AND email IS NOT NULL;
SELECT eo_attach_touch('tbl_user');

-- 登入方式。basic 的 bcrypt 雜湊在 provider_user_access_info.password（照 az 原版）。
CREATE TABLE tbl_account_link (
  id bigserial PRIMARY KEY,
  provider_id text NOT NULL,
  provider_user_id text NOT NULL,
  provider_user_access_info jsonb NOT NULL DEFAULT '{}',
  identifiers jsonb NOT NULL DEFAULT '{}',
  user_id bigint NOT NULL REFERENCES tbl_user(id),
  data jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at timestamptz
);
CREATE UNIQUE INDEX tbl_account_link_provider_uq ON tbl_account_link (provider_id, lower(provider_user_id)) WHERE deleted_at IS NULL;
CREATE INDEX tbl_account_link_user_idx ON tbl_account_link (user_id);
SELECT eo_attach_touch('tbl_account_link');

-- 伺服器端 session。token 只存 sha256；閒置期限滑動、絕對期限固定。
CREATE TABLE tbl_user_session (
  id bigserial PRIMARY KEY,
  token_hash text NOT NULL UNIQUE,
  user_id bigint NOT NULL REFERENCES tbl_user(id),
  account_link_id bigint REFERENCES tbl_account_link(id),
  created_at timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz NOT NULL,
  absolute_expires_at timestamptz NOT NULL,
  revoked_at timestamptz,
  user_agent text,
  ip text
);
CREATE INDEX tbl_user_session_user_idx ON tbl_user_session (user_id);

CREATE TABLE tbl_organization (
  id bigserial PRIMARY KEY,
  unique_name text NOT NULL,
  name text NOT NULL,
  -- 'team'＝一般組織；'personal'＝每個使用者自動擁有的個人組織。
  kind text NOT NULL DEFAULT 'team',
  personal_user_id bigint REFERENCES tbl_user(id),
  data jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at timestamptz
);
CREATE UNIQUE INDEX tbl_organization_unique_name_uq ON tbl_organization (unique_name) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX tbl_organization_personal_uq ON tbl_organization (personal_user_id) WHERE deleted_at IS NULL AND personal_user_id IS NOT NULL;
SELECT eo_attach_touch('tbl_organization');

CREATE TABLE tbl_project (
  id bigserial PRIMARY KEY,
  organization_id bigint NOT NULL REFERENCES tbl_organization(id),
  type text NOT NULL DEFAULT 'general',
  -- 每個組織恰有一個 lobby（is_default）：組織成員一律加入、不能退出。
  is_default boolean NOT NULL DEFAULT false,
  unique_name text NOT NULL,
  name text NOT NULL,
  data jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at timestamptz
);
CREATE UNIQUE INDEX tbl_project_unique_name_uq ON tbl_project (organization_id, unique_name) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX tbl_project_default_uq ON tbl_project (organization_id) WHERE deleted_at IS NULL AND is_default;
SELECT eo_attach_touch('tbl_project');

CREATE TABLE mn_user_organization (
  id bigserial PRIMARY KEY,
  user_id bigint NOT NULL REFERENCES tbl_user(id),
  organization_id bigint NOT NULL REFERENCES tbl_organization(id),
  -- 'owner'＝建立者；'member'＝其他人。權限一律看 RBAC grant，不看這欄。
  role text NOT NULL DEFAULT 'member',
  labels jsonb NOT NULL DEFAULT '[]',
  data jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at timestamptz
);
CREATE UNIQUE INDEX mn_user_organization_uq ON mn_user_organization (user_id, organization_id) WHERE deleted_at IS NULL;
CREATE INDEX mn_user_organization_org_idx ON mn_user_organization (organization_id);
SELECT eo_attach_touch('mn_user_organization');

CREATE TABLE mn_user_project (
  id bigserial PRIMARY KEY,
  user_id bigint NOT NULL REFERENCES tbl_user(id),
  project_id bigint NOT NULL REFERENCES tbl_project(id),
  role text NOT NULL DEFAULT 'member',
  labels jsonb NOT NULL DEFAULT '[]',
  data jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at timestamptz
);
CREATE UNIQUE INDEX mn_user_project_uq ON mn_user_project (user_id, project_id) WHERE deleted_at IS NULL;
CREATE INDEX mn_user_project_project_idx ON mn_user_project (project_id);
SELECT eo_attach_touch('mn_user_project');

-- 組織與專案邀請合成一張表，scope 區分。
CREATE TABLE tbl_invitation (
  id bigserial PRIMARY KEY,
  scope text NOT NULL CHECK (scope IN ('organization', 'project')),
  organization_id bigint NOT NULL REFERENCES tbl_organization(id),
  project_id bigint REFERENCES tbl_project(id),
  inviter_id bigint NOT NULL REFERENCES tbl_user(id),
  -- 被邀請者已註冊時填；未註冊時只有 email，接受時才綁上。
  invitee_id bigint REFERENCES tbl_user(id),
  invitee_email text NOT NULL,
  invite_role_key text NOT NULL,
  status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'accepted', 'declined', 'revoked', 'expired')),
  token text NOT NULL UNIQUE,
  responded_at timestamptz,
  expires_at timestamptz NOT NULL,
  data jsonb NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  deleted_at timestamptz,
  CHECK ((scope = 'project') = (project_id IS NOT NULL))
);
CREATE INDEX tbl_invitation_org_idx ON tbl_invitation (organization_id);
CREATE INDEX tbl_invitation_email_idx ON tbl_invitation (lower(invitee_email));
-- 同一個人在同一個入口只能有一筆 pending。
CREATE UNIQUE INDEX tbl_invitation_pending_uq ON tbl_invitation (scope, organization_id, COALESCE(project_id, 0), lower(invitee_email))
  WHERE deleted_at IS NULL AND status = 'pending';
SELECT eo_attach_touch('tbl_invitation');
