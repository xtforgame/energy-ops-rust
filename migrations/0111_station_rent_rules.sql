-- Reusable operational rules are independent from lease-document completeness.
CREATE TABLE tbl_energy_station_rent_rule (
 id bigserial PRIMARY KEY,
 station_id bigint NOT NULL REFERENCES tbl_energy_station(id),
 organization_id bigint NOT NULL REFERENCES tbl_organization(id),
 version integer NOT NULL DEFAULT 1 CHECK(version>0),
 valid_from date, valid_to date,
 status text NOT NULL CHECK(status IN ('pending','confirmed')),
 rule jsonb NOT NULL,
 lease_evidence jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 CHECK(valid_to IS NULL OR valid_from IS NULL OR valid_to>=valid_from),
 CHECK(status<>'confirmed' OR valid_from IS NOT NULL)
);
CREATE INDEX energy_station_rent_rule_station ON tbl_energy_station_rent_rule(organization_id,station_id);
CREATE TABLE tbl_energy_station_rent_rule_revision (
 rule_id bigint NOT NULL REFERENCES tbl_energy_station_rent_rule(id),
 version integer NOT NULL,
 rule jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(rule_id,version)
);
