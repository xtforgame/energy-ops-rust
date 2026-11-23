-- G4（2026-11-23 使用者拍板）：平台審核通過不再逐張算應付（租金改走週期試算 rent_cycle）。
-- 已安裝的庫啟動時自動換版：對每一份 key='bill-approval'、未刪、published，同時有 platform-review
-- 節點與 service_key='rating.computePayout' 節點（G1 形狀）的定義，插一份拿掉 calc 節點的新版本，
-- 把指到舊版的綁定改指新版（效期不動），已存在的執行鎖在自己那一版不受影響。
-- 新庫：這支在 seed 之前跑，那時還沒有定義，整段是 no-op。

CREATE TEMP TABLE IF NOT EXISTS _g4_node_map (old_id bigint PRIMARY KEY, new_id bigint) ON COMMIT DROP;

DO $$
DECLARE
  def RECORD;
  new_def_id bigint;
  new_version int;
  calc_id bigint;
  node RECORD;
  new_node_id bigint;
BEGIN
  FOR def IN
    SELECT d.id, d.organization_id, d.key, d.name, d.description, d.subject_type,
           d.abort_enabled, d.abort_approval_rule, d.published_by_user_id
      FROM tbl_energy_workflow_definition d
     WHERE d.key = 'bill-approval'
       AND d.deleted_at IS NULL
       AND d.status = 'published'
       AND EXISTS (
             SELECT 1 FROM tbl_energy_workflow_node n
              WHERE n.definition_id = d.id AND n.deleted_at IS NULL AND n.node_key = 'platform-review'
           )
       AND EXISTS (
             SELECT 1 FROM tbl_energy_workflow_node n
              WHERE n.definition_id = d.id AND n.deleted_at IS NULL AND n.service_key = 'rating.computePayout'
           )
     ORDER BY d.organization_id, d.version
  LOOP
    DELETE FROM _g4_node_map;

    SELECT COALESCE(MAX(version), 0) + 1 INTO new_version
      FROM tbl_energy_workflow_definition
     WHERE organization_id = def.organization_id AND key = def.key AND deleted_at IS NULL;

    INSERT INTO tbl_energy_workflow_definition
        (organization_id, key, version, parent_version_id, name, description, status, published_at,
         published_by_user_id, subject_type, abort_enabled, abort_approval_rule)
    VALUES (def.organization_id, def.key, new_version, def.id, def.name,
            COALESCE(def.description, '') || '（G4：拿掉計算應付）', 'published', clock_timestamp(),
            def.published_by_user_id, def.subject_type, def.abort_enabled, def.abort_approval_rule)
    RETURNING id INTO new_def_id;

    SELECT n.id INTO calc_id
      FROM tbl_energy_workflow_node n
     WHERE n.definition_id = def.id AND n.deleted_at IS NULL AND n.service_key = 'rating.computePayout'
     LIMIT 1;

    -- 複製節點（排除 calc），記下舊 id → 新 id。
    FOR node IN
      SELECT * FROM tbl_energy_workflow_node
       WHERE definition_id = def.id AND deleted_at IS NULL AND id <> calc_id
    LOOP
      INSERT INTO tbl_energy_workflow_node
          (definition_id, node_key, name, allows_subject_edit, node_type, assignment_rule, allow_self_approval,
           revertable, allow_jump_target, parallel, join_policy, quorum_count, join_node_key, service_key, layout,
           requirements, data)
      VALUES
          (new_def_id, node.node_key, node.name, node.allows_subject_edit, node.node_type, node.assignment_rule,
           node.allow_self_approval, node.revertable, node.allow_jump_target, node.parallel, node.join_policy,
           node.quorum_count, node.join_node_key, node.service_key, node.layout, node.requirements, node.data)
      RETURNING id INTO new_node_id;

      INSERT INTO _g4_node_map (old_id, new_id) VALUES (node.id, new_node_id);
    END LOOP;

    -- 複製動作（node_id 經 map 對到新節點；calc 的動作沒有對應的新節點，自然被 map 濾掉）。
    INSERT INTO tbl_energy_workflow_action
        (definition_id, node_id, action_key, label_key, kind, target_mode, target_node_key, who, requires_reason,
         locks_revert, side_effect_key, state_key, sort_order, data)
    SELECT new_def_id, m.new_id, a.action_key, a.label_key, a.kind, a.target_mode, a.target_node_key, a.who,
           a.requires_reason, a.locks_revert, a.side_effect_key, a.state_key, a.sort_order, a.data
      FROM tbl_energy_workflow_action a
      JOIN _g4_node_map m ON m.old_id = a.node_id
     WHERE a.definition_id = def.id AND a.deleted_at IS NULL;

    -- 不碰 calc 的轉移照抄。
    INSERT INTO tbl_energy_workflow_transition
        (definition_id, from_node_key, to_node_key, kind, label, condition, requires_reason, sort_order, data)
    SELECT new_def_id, t.from_node_key, t.to_node_key, t.kind, t.label, t.condition, t.requires_reason, t.sort_order, t.data
      FROM tbl_energy_workflow_transition t
     WHERE t.definition_id = def.id AND t.deleted_at IS NULL
       AND t.from_node_key <> 'calc' AND t.to_node_key <> 'calc';

    -- x→calc→y 合成一條 x→y：x→calc 的屬性（sort_order 等）配上 calc→y 的 to_node_key。
    INSERT INTO tbl_energy_workflow_transition
        (definition_id, from_node_key, to_node_key, kind, label, condition, requires_reason, sort_order, data)
    SELECT new_def_id, t1.from_node_key, t2.to_node_key, t1.kind, t1.label, t1.condition, t1.requires_reason, t1.sort_order, t1.data
      FROM tbl_energy_workflow_transition t1
      JOIN tbl_energy_workflow_transition t2
        ON t2.definition_id = t1.definition_id AND t2.from_node_key = 'calc' AND t2.deleted_at IS NULL
     WHERE t1.definition_id = def.id AND t1.deleted_at IS NULL AND t1.to_node_key = 'calc';

    -- 綁定改指新版，效期不動；已存在的執行鎖在自己那一版，不受影響。
    UPDATE tbl_energy_workflow_binding
       SET definition_id = new_def_id
     WHERE definition_id = def.id AND deleted_at IS NULL;

  END LOOP;
END $$;
