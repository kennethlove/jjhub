-- story: e02s01
ALTER TABLE users ADD COLUMN handle CITEXT;
UPDATE users SET handle = split_part(email, '@', 1);
-- de-dupe any collisions deterministically before unique constraint
WITH d AS (
    SELECT id, row_number() OVER (PARTITION BY handle ORDER BY created_at) AS rn
    FROM users WHERE handle IS NOT NULL
)
UPDATE users u SET handle = u.handle || '-' || d.rn FROM d WHERE u.id = d.id AND d.rn > 1;
ALTER TABLE users ALTER COLUMN handle SET NOT NULL;
ALTER TABLE users ADD CONSTRAINT users_handle_unique UNIQUE (handle);
