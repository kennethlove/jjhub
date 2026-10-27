-- story: e22s02 — exact-match lookup for token authentication.
--
-- WHY: tokens are 24-byte CSPRNG values, so the stored verification
-- secret may be a fast hash; the argon2 full-scan that authenticate
-- ran cost ~270ms per active row (measured: 35-40s per git request at
-- 130 shared-DB rows) and starved the transport. sha256 of the
-- plaintext is indexed and constant-time-ish to find; rows minted
-- before this migration keep lookup_hash NULL and are still honored
-- by the legacy argon2 scan until rotated out.

ALTER TABLE tokens ADD COLUMN lookup_hash BYTEA;
CREATE UNIQUE INDEX tokens_lookup_hash_idx ON tokens (lookup_hash);
