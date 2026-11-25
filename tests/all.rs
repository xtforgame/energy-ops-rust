//! 聚合測試 target：所有整合測試掛在這裡，只編一次。

// 測試佈景就地解 tuple、參數多是常態。
#![allow(clippy::type_complexity, clippy::too_many_arguments)]

#[macro_use]
mod support;
mod platform;
mod energy_workflow_skeleton;
mod energy_workflow_2c;
mod energy_default_flow;
mod energy_contract_flows;
mod energy_mode;
mod energy_execution_list;
mod energy_rating_bindings;
mod energy_master_data;
mod energy_org_audit;
mod energy_http;
mod energy_demo_seed;
mod tpr;
mod energy_intake;
mod energy_recognition;
mod energy_attachments;
mod energy_reminders;
mod energy_step_requirements;
mod energy_easyflow;
mod energy_rent_cycle;
mod energy_bill_rent_separation;
mod energy_station_rent_rules;
