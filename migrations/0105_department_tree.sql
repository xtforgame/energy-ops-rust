-- 部門上下層（卡 C／G01）：鄰接表，parent_id 指向同組織的另一個部門。
-- 讀整棵樹、擋循環都用遞迴 CTE（crates/eo-energy/src/org/departments.rs）。
-- 部門本身是軟刪除（deleted_at），ON DELETE SET NULL 只接「列真的被刪」那一刻；
-- 軟刪除子部門改掛上層的邏輯在 delete_department() 裡另外處理。
ALTER TABLE tbl_energy_department ADD COLUMN parent_id bigint REFERENCES tbl_energy_department(id) ON DELETE SET NULL;
CREATE INDEX tbl_energy_department_parent_idx ON tbl_energy_department (parent_id) WHERE deleted_at IS NULL;
