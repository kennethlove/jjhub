Design three new pages for jjhub, following the landing page design
language from this project. This is design-only work: produce ONE
self-contained HTML file per page, written to disk in the project
directory. Do not modify any other files.

You MUST actually write the files before finishing:
  auth.html       — Log in and Register screens
  repo.html       — The public repository landing page
  preferences.html — The signed-in user's preferences/settings page

If you stop before writing each file, the run fails.

=========================================================
THE DESIGN SYSTEM (repo-root DESIGN.md, verbatim tokens)
=========================================================
Colors: primary #16211C, secondary #4A574F, tertiary #157F52 (sole
accent), tertiary-hover #116944, tertiary-pressed #0D5436, neutral
#F4F6F4 (page bg), surface #FBFCFB, surface-sunken #ECEFEC,
surface-inverse #1D2A24, border #DCE2DD, on-accent #F7FAF8,
on-inverse #E7ECE8, error #A32218, error-surface #FBEDEC.
No pure #fff or #000. No gradients. No glow. No second accent.

Typography: IBM Plex Sans (400/500/600) for UI and prose; IBM Plex
Mono (400/500) for every ID, hash, revision, branch name, token, and
code sample. System fallbacks behind both.

Type scale (modular, no other sizes): headline-1 49px, headline-2
39px, headline-3 31px, headline-4 25px, body-lg 20px, body 16px,
body-sm 14px, label 14px, label-sm 12px, data 14px mono, data-sm
12px mono.

Spacing: 8px grid — 4/8/16/24/32/48/64 only. Max content width
72rem, page margins 32px desktop.

Radius: 0 (ledger rules), 4px (inputs, badges), 8px (cards, code
blocks), 12px (large panels), full (pills).

Elevation: hairline borders (#DCE2DD) and background shifts only.
No box-shadows on page content.

States: every control declares default, hover, focus-visible (2px
accent outline, 2px offset), active, disabled. Motion 120–180ms
ease-out, disabled under prefers-reduced-motion.

Signature: hairline-ruled rows; 12-character mono change-ids and
hashes; the commit-ledger look, never a SaaS dashboard.

=========================================================
THE SHARED CHROME (reuse across all three pages)
=========================================================
Header: wordmark "jjhub" left (mono, ink #16211C), nav right.
Footer: inverse band #1D2A24, on-inverse #E7ECE8, wordmark left;
right side "Apache License 2.0 · jj-vcs.dev · GitHub".

=========================================================
PAGE 1 — auth.html
=========================================================
One file containing BOTH the Log in screen and the Register screen,
stacked vertically, each a centered card (max-width ~26rem,
surface #FBFCFB, hairline border, radius 8px) on the neutral page.

Log in: h1 "Log in", email + password fields, primary green button
"Log in", a quiet line under it "New here? Create an account" that
links to the register card. Real error state shown as a designed
example: an alert strip on error-surface #FBEDEC with error text
#A32218 — copy "Email or password is incorrect."

Register: h1 "Create your account", email + password + confirm
password fields, password hint line in body-sm secondary ("At least
10 characters."), primary button "Create account", quiet line "Already
have an account? Log in". Show one validation-error example on the
confirm field: border in error #A32218, helper text "Passwords do
not match."

Between the two cards, nothing — they are two states of one flow.

=========================================================
PAGE 2 — repo.html
=========================================================
The visitor view of a public repository, in the same language as the
landing page hero and ledger.

Header block: owner/name as h1 with the owner in secondary and name
in ink ("kennethlove / jjhub" — slash in border gray), a one-line
description ("A forge built around Jujutsu."), and a meta row of
mono data-sm: visibility "public", "trunk" as the default branch,
a "Last push 2 hours ago" relative time.

A right-side panel (surface, hairline, radius 8) titled "Add this
repository" with a mono clone line
  git clone http://jjhub.example/kennethlove/jjhub.git
and, below it, an "Install SSH key" hint line. A "Generate clone
token" secondary button. Selectable code text.

Files section: hairline-ruled ledger table of the root tree.
Columns: Name (mono), Type (dir/file, secondary), Size (mono,
right-aligned, tabular-nums). 8–10 realistic rows for jjhub's own
repo: .github/ (dir), crates/ (dir), docs/ (dir), specs/ (dir),
.gitignore, Cargo.lock, Cargo.toml, Dockerfile, docker-compose.yml,
README.md.

Open merge requests section: ledger rows. Columns: change-id (mono,
12 chars), title (body), branch (mono, secondary), status pill
(Open / Merged, same pills as the landing ledger). 4 rows with real
titles: "Rate-limit policy on login", "Push admission audit trail",
"Revset search backend", "Conflict-tolerant push storage" (last one
Merged).

Protected-refs note: a quiet inline strip under the files heading:
"Trunk is protected — direct pushes are rejected." in body-sm,
secondary, with a small mono accent on the word trunk.

=========================================================
PAGE 3 — preferences.html
=========================================================
The signed-in user's preferences page. Signed-in chrome: wordmark
left, nav right shows "Repos · Settings · kennethlove · Log out".

h1 "Your preferences". Two panels stacked, each surface, hairline,
radius 12px, 32px padding.

Panel 1 "Account": read-only email shown as a definition row
(label left secondary, value right in mono), a "Change password"
secondary button, and a danger zone at the panel bottom separated
by a hairline: "Delete account" in error #A32218 with body-sm
helper "This removes your account and all repositories. This cannot
be undone."

Panel 2 "Access tokens": the one-time reveal state, designed as a
real moment — a success strip (surface with accent hairline left
border) containing:
  "Copy this token now — it will not be shown again." (body-sm)
  a mono token value kqh7f2m9x4vr8pql in a sunken code field with
  a "Copy" secondary button beside it.
Below the strip, the generate form: a "Label" text input
(pre-filled example "ci-deploy") and a primary "Generate token"
button.
Then the token list as a ledger table: Label (body), Prefix (mono,
secondary), Created (mono data-sm). 3 rows: "ci-deploy" kqh7f2m…,
"laptop" mzv8k1p…, "old-clone" 9xwr4dd….

=========================================================
ANTI-SLOP RULES (hard)
=========================================================
- Tokens only, every value from the system above.
- No gradients, glow, glassmorphism, floating orbs, purple anything.
- No box-shadows.
- Full states on inputs and buttons; visible focus rings.
- prefers-reduced-motion fallback on every transition.
- Real copy, no Lorem ipsum, no "Acme".
- One accent color, used for primary actions and small marks only.

Write auth.html, repo.html, preferences.html to the project
directory now.
