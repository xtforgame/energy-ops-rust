//! 能源端點的閘（permission 任一即可；`platform:rbac:manage` 由 `Rbac::authorize_org` 自動加上）。
//! 照 App 5 `AcEnergyOpsController.ts` 的常數。

pub const GOVERN_ORG: &[&str] = &["energy:department:manage", "energy:member:manage"];
pub const ASSIGN_ROLE: &[&str] = &["energy:member:manage"];
pub const FLOW_ACT: &[&str] = &["energy:flow:act"];
pub const FLOW_VIEW: &[&str] = &["energy:flow:view", "energy:flow:view-all"];
pub const FLOW_VIEW_ALL: &[&str] = &["energy:flow:view-all"];
pub const FLOW_ADMIN: &[&str] = &["energy:flow:admin"];
pub const ABORT_APPROVE: &[&str] = &["energy:flow:abort-approve"];
pub const DELEGATION_MANAGE: &[&str] = &["energy:delegation:manage"];
pub const AUDIT_VIEW: &[&str] = &["energy:audit:view"];
pub const WORKFLOW_MANAGE: &[&str] = &["energy:workflow:manage"];
pub const RATING_MANAGE: &[&str] = &["energy:rating:manage"];
pub const ASSIGN_PRIVILEGED: &[&str] = &["energy:role:assign-privileged"];
pub const ATTACHMENT_UPLOAD: &[&str] = &["energy:attachment:upload"];
pub const ATTACHMENT_DOWNLOAD: &[&str] = &["energy:attachment:download"];
pub const ATTACHMENT_UNLINK: &[&str] = &["energy:attachment:unlink"];
pub const PAYOUT_VIEW: &[&str] = &["energy:payout:view"];
pub const PAYOUT_RECOMPUTE: &[&str] = &["energy:payout:recompute"];

/// 組織治理（「最後一位治理者」防呆看這兩個）。
pub const GOVERNANCE: &[&str] = &["energy:govern:org", "platform:rbac:manage"];
