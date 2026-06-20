# Deploying money·data

The whole app — Rust/axum API **and** the built Vite frontend — packages into a single
`linux/amd64` Docker image that binds `0.0.0.0:$PORT`. It needs a Postgres database
(`DATABASE_URL`); balances/ledger/refunds are persisted there, so they survive restarts,
cold starts, and multiple instances.

**How it deploys:** a push to `main` triggers `.github/workflows/deploy.yml` — Cloud Build
builds the Dockerfile in the cloud (no local Docker) and deploys to Google Cloud Run, backed
by a free Neon Postgres. See **[ARCHITECTURE.md](ARCHITECTURE.md)** for the rationale.

---

## The four GitHub secrets

The workflow reads four secrets. Below is how to obtain each value, then how to add them to
GitHub **by hand** through the web UI.

| Secret | What it is | How to get the value |
| ------ | ---------- | -------------------- |
| `GCP_PROJECT_ID` | Your Google Cloud project **id** (not the display name) | GCP Console top bar → project picker, or `gcloud projects list` |
| `GCP_SA_KEY` | JSON key for the `gh-deployer` service account | Create the SA + key (steps below); the secret value is the **entire contents** of the `.json` file |
| `DATABASE_URL` | Neon pooled connection string | Neon dashboard (steps below) |
| `JWT_SECRET` | Signs auth session tokens | `openssl rand -hex 32` |

### 1. `GCP_PROJECT_ID`

In the [GCP Console](https://console.cloud.google.com) the project id is shown in the top-bar
project picker (or run `gcloud projects list` and copy the `PROJECT_ID` column). The project
must have **billing enabled** — you won't be charged within the Cloud Run / Neon free limits.

### 2. `GCP_SA_KEY`

This is a service account that GitHub Actions authenticates as to deploy.

**Console route (fully manual):**
1. GCP Console → **IAM & Admin → Service Accounts → Create service account**.
2. Name it `gh-deployer` → **Create and continue**.
3. Grant these five roles, then **Done**:
   - `Cloud Run Admin`
   - `Cloud Build Editor`
   - `Artifact Registry Administrator`
   - `Storage Admin`
   - `Service Account User`
4. Open the new service account → **Keys → Add key → Create new key → JSON**. A `.json` file
   downloads.
5. Open that file; its **entire contents** are the `GCP_SA_KEY` value to paste into GitHub.

**Shortcut:** `bash scripts/setup-gcp.sh <PROJECT_ID>` does all of the above (enables APIs,
creates the SA, grants the roles, writes `gh-sa-key.json`). Paste that file's contents as the
secret, then `rm gh-sa-key.json`.

> Treat this JSON as a credential — don't commit it. Delete the local copy once it's in GitHub.

### 3. `DATABASE_URL`

1. Sign up at **[neon.tech](https://neon.tech)** (no credit card) → **New Project** (pick a
   region near your users).
2. In **Connection Details**, copy the **Pooled** connection string — use the **pooled**
   (`...-pooler...`) host so Cloud Run's many short-lived instances stay under Neon's
   connection limit. It looks like:
   ```
   postgres://USER:PASSWORD@ep-xxx-pooler.REGION.aws.neon.tech/neondb?sslmode=require
   ```
   Keep `?sslmode=require` (the backend connects over TLS).

The app runs its own migrations and seeds demo data on first boot — no manual SQL needed.

### 4. `JWT_SECRET`

```bash
openssl rand -hex 32
```

Set it **once** and keep it stable — changing it logs everyone out. The app refuses to boot
without it.

### Add them to GitHub (web UI)

1. Repo → **Settings → Secrets and variables → Actions**.
2. Select the **Secrets** tab → **Repository secrets** → **New repository secret**.
3. Add each of the four: `GCP_PROJECT_ID`, `GCP_SA_KEY`, `DATABASE_URL`, `JWT_SECRET`.

> ⚠️ **Use _Repository_ secrets, not _Environment_ secrets.** Environment secrets are only
> readable by a job that declares `environment: <name>`. The workflow doesn't, so
> environment-scoped secrets resolve to **empty strings** — which is exactly the `❌ … is
> EMPTY` / `google-github-actions/auth failed` symptom. (If you *want* an environment, add an
> `environment:` key to the `deploy` job in `.github/workflows/deploy.yml` instead.)

---

## Deploy

Push to `main`:

```bash
git push origin main
```

Watch the run under the repo's **Actions** tab. The **Verify secrets are present** step should
print `✅` for all four; the **Deploy to Cloud Run** step prints your URL:

```
https://money-data-xxxxxxxx-uc.a.run.app
```

(Once auth works, the diagnostic `Verify secrets are present` step in the workflow can be
removed.)

### Verify

```bash
curl https://<your-run-url>/api/health        # {"service":"money-data","status":"ok"}
```

Open the URL — the dark dashboard loads, KPIs animate, the ledger renders. Create a payment
link, push again to force a fresh revision, and confirm the data is still there — that's the
durable-DB win.

### Ongoing

`git push` to `main` → Actions → Cloud Run redeploys. Env vars persist across revisions.

---

## Runtime env vars (reference)

These are set on the Cloud Run service by the workflow (`DATABASE_URL`, `JWT_SECRET`).

| Var | Purpose |
| --- | ------- |
| `DATABASE_URL` | Postgres connection (Neon pooled string, `?sslmode=require`). **Required.** |
| `JWT_SECRET` | Signs auth session tokens. **Required — the app refuses to boot without it.** Set once, keep stable; changing it logs everyone out. |
| `EXPOSE_MAGIC_LINK` | **Leave UNSET in production.** Local-dev only: returns the sign-in link in the API response. In production it would let anyone request a link for a known email and sign in as them. |

**Signing in on the deployed site (no email yet):** auth is passwordless magic-link, and with
`EXPOSE_MAGIC_LINK` unset the link is **not** returned to the browser — it's written to the
server logs. The seeded admin `digitallymba@gmail.com` exists on first boot. To sign in,
request the link in the UI, then read it from the logs:

```bash
gcloud run services logs read money-data --region us-central1 --limit 20 | grep magic-link
```

Open that `#/auth/verify?token=…` path on your deployed URL. (Swap to real email delivery, or
front the site with IAP/Basic-Auth, before opening it up to others.)

---

## Operate

**Logs**

```bash
gcloud run services logs read money-data --region us-central1
```

**Custom domain (free, optional)** — Cloud Run → *Manage Custom Domains*, or front it with
Cloudflare DNS/CDN.

**Tear down**

```bash
gcloud run services delete money-data --region us-central1   # then delete the Neon project
```

**Cost** — Cloud Run (2M req/mo, scale-to-zero) + Neon (free, no card) = **$0** at validation
traffic. Only overage risk is pennies of Artifact Registry storage for built images.
