-- Preserve existing identities for Taipower, split independent retailer documents.
-- Existing retailer keys are updated once; new ingestion uses the same prefix.
UPDATE tbl_energy_bill
SET business_key = 'retailer|' || COALESCE(template_id, 'unknown') || '|' || business_key
WHERE doc_type = 'retailerSale' AND business_key LIKE 'bill|%';
