-- relay-x egress control (Phase: egress-control)
--
-- Adds `proxy_url` to lanes so `egress = 'masked'` can route through an
-- HTTP CONNECT or SOCKS5 proxy. `egress` itself already exists on `lanes`
-- (001_initial.sql, default 'direct'); this migration only adds the proxy
-- address field and an index for lookup.
--
-- No enum constraint on egress: deployments may carry future values, and
-- the gateway degrades unknown values to direct (fail-open) while masked
-- without proxy_url degrades to direct with a warning.

ALTER TABLE lanes ADD COLUMN IF NOT EXISTS proxy_url TEXT;

-- Proxy URL lookups go through the lane id; an index mirrors the
-- base_url usage pattern in repositories.
CREATE INDEX IF NOT EXISTS idx_lanes_proxy_url ON lanes(proxy_url);
