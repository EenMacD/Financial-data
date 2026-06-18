-- External Trip ID (the operator's own reference) and the Ryft payment-session
-- id backing each link.
ALTER TABLE payment_links ADD COLUMN trip_id TEXT;
ALTER TABLE payment_links ADD COLUMN ryft_session_id TEXT;
