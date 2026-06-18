-- Auth + multi-tenancy. Each travel business is a tenant; users sign in with
-- passwordless magic links (admins span all tenants, tenant users are pinned to
-- one). auth_tokens powers both magic-link sign-in and invites.

CREATE TABLE tenants (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    region              TEXT NOT NULL DEFAULT '',
    settlement_currency TEXT NOT NULL DEFAULT 'USD',
    registration_number TEXT,
    country             TEXT,
    contact_name        TEXT,
    contact_email       TEXT,
    ryft_subaccount_id  TEXT,
    onboarding_status   TEXT NOT NULL DEFAULT 'pending',
    onboarding_url      TEXT,
    kyb_details         JSONB,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE users (
    id         TEXT PRIMARY KEY,
    email      TEXT NOT NULL UNIQUE,
    name       TEXT NOT NULL,
    role       TEXT NOT NULL,                  -- 'admin' | 'tenant'
    tenant_id  TEXT REFERENCES tenants(id),    -- NULL for platform admins
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE auth_tokens (
    token      TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,                  -- 'magic' | 'invite'
    email      TEXT NOT NULL,
    name       TEXT,
    role       TEXT,                           -- invites: role to grant
    tenant_id  TEXT REFERENCES tenants(id),    -- invites: tenant to attach
    expires_at TIMESTAMPTZ NOT NULL,
    used_at    TIMESTAMPTZ
);

-- Tenant scoping on the transactional tables.
ALTER TABLE wallets            ADD COLUMN tenant_id TEXT;
ALTER TABLE payment_links      ADD COLUMN tenant_id TEXT;
ALTER TABLE refunds            ADD COLUMN tenant_id TEXT;
ALTER TABLE settlement_batches ADD COLUMN tenant_id TEXT;
ALTER TABLE platform_revenue   ADD COLUMN tenant_id TEXT;

-- Backfill any pre-existing single-tenant rows so the constraints below apply.
-- No-op on a fresh database (the app seeds afterwards with real tenant ids).
INSERT INTO tenants (id, name, region, settlement_currency, onboarding_status)
SELECT 'tnt_legacy', 'Legacy Operator', 'Unknown', 'USD', 'verified'
WHERE EXISTS (SELECT 1 FROM wallets)
   OR EXISTS (SELECT 1 FROM payment_links)
   OR EXISTS (SELECT 1 FROM platform_revenue);

UPDATE wallets            SET tenant_id = 'tnt_legacy' WHERE tenant_id IS NULL;
UPDATE payment_links      SET tenant_id = 'tnt_legacy' WHERE tenant_id IS NULL;
UPDATE refunds            SET tenant_id = 'tnt_legacy' WHERE tenant_id IS NULL;
UPDATE settlement_batches SET tenant_id = 'tnt_legacy' WHERE tenant_id IS NULL;
UPDATE platform_revenue   SET tenant_id = 'tnt_legacy' WHERE tenant_id IS NULL;

-- Wallets become per-tenant: PK (tenant_id, currency).
ALTER TABLE wallets DROP CONSTRAINT wallets_pkey;
ALTER TABLE wallets ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE wallets ADD PRIMARY KEY (tenant_id, currency);

ALTER TABLE payment_links      ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE refunds            ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE settlement_batches ALTER COLUMN tenant_id SET NOT NULL;

-- platform_revenue: one row per tenant instead of a single global row.
ALTER TABLE platform_revenue DROP CONSTRAINT single_row;
ALTER TABLE platform_revenue DROP COLUMN id;            -- also drops the old PK
ALTER TABLE platform_revenue ALTER COLUMN tenant_id SET NOT NULL;
ALTER TABLE platform_revenue ADD PRIMARY KEY (tenant_id);

CREATE INDEX idx_payment_links_tenant ON payment_links (tenant_id, created_at DESC);
CREATE INDEX idx_settlement_batches_tenant ON settlement_batches (tenant_id, created_at DESC);
CREATE INDEX idx_users_email ON users (email);
