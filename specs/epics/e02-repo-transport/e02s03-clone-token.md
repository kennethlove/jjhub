# e02s03 — Clone snippet carries the viewer's access token

Reported after MVP review: the copy-paste remote snippet on the repo
page shows `<TOKEN>`, forcing a detour to settings.

## Acceptance

1. Signed-in viewers get a Generate token button under the snippet.
   Submitting it creates a token (same one-time display semantics as
   settings) and redirects back to the same repo page.
2. With the one-time `new_token` cookie present, the snippet renders
   `x-access-token:<their token>@…` so the `jj git remote add` line
   works as pasted.
3. Anonymous viewers keep the `<TOKEN>` placeholder and no button.
   Tokens stay hashed at rest; plaintext exists only in the one-time
   cookie, exactly like the settings flow.
