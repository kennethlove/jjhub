# e01s03 — Signup confirmation, auth error pages, logout control

Reported after MVP review: passwords are never confirmed at sign-up,
failed login shows an empty 401, and no page offers a logout control.

## Acceptance

1. Register form has a confirm-password field. Mismatched confirmation
   returns 422, renders the form with `passwords do not match`, and
   creates no user row.
2. Login failure returns 401 with the login form and a visible message
   (`email or password is incorrect`). Duplicate email returns 409 with
   the register form and `email already registered`. Weak password /
   invalid email return 422 with the register form and the domain error
   text. Status codes stay what the existing tests assert.
3. Every authenticated page's nav contains a Log out button: POST
   `/logout` with the hidden `_csrf` synchronizer token. Logged-out nav
   is unchanged.

## Out of scope

- Rate limiting login attempts (no epic owns it; beta).
- Session revocation beyond cookie clear (single-node, in-memory check).
