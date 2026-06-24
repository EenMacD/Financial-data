-- Email + password sign-in. Existing users (seeded / invited) start with no
-- password; they get one when an admin (re)creates them through "Add user" or
-- via the ADMIN_EMAIL/ADMIN_PASSWORD bootstrap on startup. A NULL hash simply
-- means that account cannot log in with a password yet.
ALTER TABLE users ADD COLUMN password_hash TEXT;
