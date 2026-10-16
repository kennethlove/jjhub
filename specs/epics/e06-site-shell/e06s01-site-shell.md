# e06s01 — Home page, nav menu, repo create form, origin snippets

## 1. Story

As a self-hoster, the site has a front door: `/` welcomes visitors with register/login, shows my repos and a create form when logged in, a persistent nav reaches every feature, and each repo page carries copy-and-run commands for wiring a local clone.

## 2. Business value

The MVP had no landing page (`/` 404s) and no UI for repo creation — the product was unusable without curl. This is the onboarding surface.

## 3. Type

feat

## 4. Risk / context

risk: P1 — context: UI (server-rendered HTML, no JS framework).

## 5. Context

`GET /`: logged-out visitors get a hero with Register/Log-in links; logged-in users get the create-repo form (posts to the existing CSRF-protected `/repos`), a list of repos they own (name + visibility + link), and links to Settings. A shared nav bar renders on all pages: brand link to `/`, and when authed — Repos (home), Settings. Repo detail pages gain an "Add this repository" block: clone URL, `jj git remote add origin …` and push commands with a `<TOKEN>` placeholder plus the real host from the request.

## 6. Requirements

#### ADDED: Home page — logged out
`GET /` shows product name, Register and Log-in links; no other chrome.

#### ADDED: Home page — logged in
Create-repo form (name + visibility), list of owned repos with links, Settings link.

#### ADDED: Nav menu
Shared nav on every HTML page: brand → `/`; authed adds Repos and Settings.

#### ADDED: Origin snippets on repo page
Repo detail shows clone URL and jj commands with `<TOKEN>` placeholder, host taken from the request (`Host` header).

## 7. Actors

Visitor, user.

## 8. Preconditions

e01–e03 shipped (sessions, repos, MR pages).

## 9. Postconditions

A new self-host can register, create a repo from the home page, and wire a local clone using only rendered instructions.

## 10. Business rules

- Create form goes through the same CSRF middleware as every POST.
- Repo list shows only repos the viewer owns (collaborator repos: out of scope here).
- Snippets never embed real token values.

## 11. Domain model touchpoints

**User**, **Repository** (list + create).

## 12. UI/UX notes

Server-rendered inline HTML consistent with existing pages; Tailwind classes as before. No JS.

## 13. API contracts

None new — `POST /repos` already exists.

## 14. Data model

None.

## 15. Non-functional requirements

Home page ≤1 query for the repo list.

## 16. Out of scope

Orgs, collaborator repo lists, search, avatars, dark mode, i18n.

## 17. Acceptance criteria

- [ ] `GET /` logged out → 200 with Register + Log-in links (test).
- [ ] `GET /` logged in → create form + owned repos listed (test).
- [ ] Nav appears on home, settings, repo pages (test).
- [ ] Repo page shows clone URL and `jj git remote add origin` snippet with `<TOKEN>` placeholder, using the request host (test).
- [ ] Create form from home page works end-to-end (303 + repo appears in list) (test).
- [ ] Preflight green.

## 18. Test scenarios (Gherkin)

```gherkin
Scenario: visitor sees the front door
  Given I am logged out
  When I GET /
  Then I see Register and Log-in links

Scenario: user creates a repo from home
  Given I am logged in
  When I submit the create form on /
  Then I am redirected to my new repo
  And the repo appears in my list on /
```

## 19. Verification script (manual)

1. Open `http://localhost:3000/` logged out — landing with register/login.
2. Register — home now shows the form and empty repo list.
3. Create `demo` — lands on repo page with copy-paste jj commands; nav visible.
4. Run the rendered commands locally — clone/push works.

## 20. Traceability

`// story: e06s01` in implementing code and tests.
