//! 能源營運的 RBAC seed：17 個 simple、5 個 complex、5 個角色（來源：`common/rbacSeeds.ts`）。
//!
//! 角色只綁 complex 那一層 ——「改得動」的關鍵。全部在 organization scope 宣告。
//! seed 只增不刪：uniqueName 一次定對，日後改名或砍角色都要一次性清理腳本。
//! `energy:ops:view`／`energy:ops:edit` 刻意不宣告：成員還不存在，空 complex 會讓 seed 驗證失敗。

use serde::Serialize;

use crate::rbac::{RbacScope, RbacSeeds};

pub const ENERGY_OPS_PLUGIN_NAME: &str = "energyOps";

/// 舊版每道門都 OR 一個 `ryxis:rbac:manage`；這裡的總鑰匙改名為 `platform:rbac:manage`，由 platform crate seed。
pub const PLATFORM_RBAC_MANAGE: &str = "platform:rbac:manage";

const SCOPE: RbacScope = RbacScope::Organization;

/// simple permission：(uniqueName, 中文顯示名)。顯示名不可洩漏 uniqueName。
pub const ENERGY_SIMPLE_PERMISSIONS: [(&str, &str); 18] = [
    // ── 組織治理 ──
    ("energy:member:manage", "能源營運：成員與角色指派"),
    ("energy:department:manage", "能源營運：部門管理"),
    ("energy:delegation:manage", "能源營運：代理規則管理"),
    // ── 主檔 ──
    ("energy:master:manage", "能源營運：主檔維護"),
    // ── 稽核 ──
    ("energy:audit:view", "能源營運：稽核紀錄檢視"),
    // ── 流程 ──
    ("energy:flow:view", "能源營運：流程檢視（我參與的）"),
    ("energy:flow:view-all", "能源營運：流程檢視（全組織）"),
    ("energy:flow:act", "能源營運：流程操作（認領／核可）"),
    ("energy:flow:admin", "能源營運：流程強制處置（解除副作用鎖／強制推進／終止）"),
    ("energy:flow:abort-approve", "能源營運：流程中斷核可"),
    // ── 系統設定 ──
    ("energy:workflow:manage", "能源營運：流程定義管理"),
    ("energy:rating:manage", "能源營運：計算策略管理"),
    ("energy:role:assign-privileged", "能源營運：指派管理者角色"),
    // ── 附件 ──
    ("energy:attachment:upload", "能源營運：附件上傳"),
    ("energy:attachment:download", "能源營運：附件下載"),
    ("energy:attachment:unlink", "能源營運：附件解除連結"),
    // ── 應付 ──
    ("energy:payout:view", "能源營運：應付紀錄檢視"),
    ("energy:payout:recompute", "能源營運：應付紀錄重算"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnergyComplexDef {
    pub unique_name: &'static str,
    pub name: &'static str,
    /// 只能是 simple（不可包 complex）；不可為空。
    pub includes: &'static [&'static str],
}

pub const ENERGY_COMPLEX_PERMISSIONS: [EnergyComplexDef; 5] = [
    EnergyComplexDef {
        unique_name: "energy:govern:org",
        name: "能源營運：組織治理",
        includes: &["energy:member:manage", "energy:department:manage", "energy:delegation:manage"],
    },
    EnergyComplexDef {
        unique_name: "energy:govern:system",
        name: "能源營運：系統設定",
        includes: &["energy:workflow:manage", "energy:rating:manage", "energy:flow:admin", "energy:role:assign-privileged"],
    },
    EnergyComplexDef {
        unique_name: "energy:oversight",
        name: "能源營運：全域檢視與稽核",
        includes: &["energy:flow:view-all", "energy:audit:view", "energy:attachment:download"],
    },
    EnergyComplexDef {
        unique_name: "energy:flow:participate",
        name: "能源營運：流程參與",
        includes: &["energy:flow:view", "energy:flow:act", "energy:attachment:upload", "energy:attachment:download"],
    },
    EnergyComplexDef {
        unique_name: "energy:finance:act",
        name: "能源營運：財務作業",
        includes: &["energy:payout:view", "energy:payout:recompute"],
    },
];

/// 前端下拉／邀請 preset 用的角色描述（uniqueName 不進 UI 文案）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnergyRoleDef {
    pub unique_name: &'static str,
    /// UI 顯示名（＝ seed 進 DB 的角色名）。
    pub name: &'static str,
    /// 一句話說明，下拉選項的副標。
    pub description: &'static str,
    pub permissions: &'static [&'static str],
}

pub const ENERGY_ROLE_DEFS: [EnergyRoleDef; 5] = [
    EnergyRoleDef {
        unique_name: "energy:admin",
        name: "系統管理員",
        description: "全部權限，含系統設定與指派管理者角色",
        permissions: &[
            "energy:govern:system", "energy:govern:org", "energy:oversight",
            "energy:flow:participate", "energy:finance:act", "energy:flow:abort-approve",
            "energy:master:manage",
        ],
    },
    EnergyRoleDef {
        unique_name: "energy:cxo",
        name: "高階主管",
        description: "可調整組織架構與他人權限，但不能指派管理者角色",
        permissions: &[
            "energy:govern:org", "energy:oversight",
            "energy:flow:participate", "energy:finance:act", "energy:flow:abort-approve",
        ],
    },
    EnergyRoleDef {
        unique_name: "energy:operator",
        name: "業務承辦",
        description: "參與流程：建檔、送審、核可自己關卡的單",
        permissions: &["energy:flow:participate"],
    },
    EnergyRoleDef {
        unique_name: "energy:finance",
        name: "財務",
        description: "參與流程，另可檢視與重算應付紀錄",
        permissions: &["energy:flow:participate", "energy:finance:act"],
    },
    EnergyRoleDef {
        unique_name: "energy:viewer",
        name: "唯讀",
        description: "看得到全組織的單與稽核紀錄，不能操作",
        permissions: &["energy:oversight"],
    },
];

/// 只有持 `energy:role:assign-privileged` 的人能指派／移除這些角色。不做角色階層引擎。
pub const PRIVILEGED_ROLES: [&str; 2] = ["energy:admin", "energy:cxo"];

/// 不能移除最後一個持有組織治理權的人。判定用 complex permission，日後加角色不必改防呆。
pub const ORG_GOVERNANCE_PERMISSION: &str = "energy:govern:org";

/// 取單一角色定義；找不到回 None（呼叫端須擋）。
pub fn get_energy_role_def(unique_name: &str) -> Option<&'static EnergyRoleDef> {
    ENERGY_ROLE_DEFS.iter().find(|r| r.unique_name == unique_name)
}

/// 是否為本 feature 管的角色（角色指派端點只認這五個）。
pub fn is_energy_role(unique_name: &str) -> bool {
    get_energy_role_def(unique_name).is_some()
}

pub fn is_privileged_energy_role(unique_name: &str) -> bool {
    PRIVILEGED_ROLES.contains(&unique_name)
}

/// 能源營運 plugin 的 seed。
pub fn energy_rbac_seeds() -> RbacSeeds {
    let mut seeds = RbacSeeds::new(ENERGY_OPS_PLUGIN_NAME);
    for (unique_name, name) in ENERGY_SIMPLE_PERMISSIONS {
        seeds = seeds.simple(SCOPE, unique_name, name);
    }
    for c in ENERGY_COMPLEX_PERMISSIONS {
        seeds = seeds.complex(SCOPE, c.unique_name, c.name, c.includes);
    }
    for r in ENERGY_ROLE_DEFS {
        seeds = seeds.role(SCOPE, r.unique_name, r.name, r.permissions);
    }
    seeds
}

/// 角色展開成 simple permission（complex 換成它包含的那幾個）。
pub fn expand_role_permissions(role_unique_name: &str) -> Vec<&'static str> {
    let Some(role) = get_energy_role_def(role_unique_name) else {
        return vec![];
    };
    role.permissions
        .iter()
        .flat_map(|p| match ENERGY_COMPLEX_PERMISSIONS.iter().find(|c| c.unique_name == *p) {
            Some(c) => c.includes.to_vec(),
            None => vec![*p],
        })
        .collect()
}
