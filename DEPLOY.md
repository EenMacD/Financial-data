# Deploying money·data

The whole app — Rust/axum API **and** the built Vite frontend — packages into a single
`linux/amd64` Docker image that binds `0.0.0.0:$PORT`. It needs a Postgres database
(`DATABASE_URL`); balances/ledger/refunds are persisted there, so they survive restarts,
cold starts, and multiple instances.

Two supported paths:

| Path | Host | Cost | When |
| ---- | ---- | ---- | ---- |
| **Free (recommended)** | Google Cloud Run + Neon Postgres | **$0** at validation traffic | Now — sharing a live URL while validating |
| Paid | Cloudflare Containers + Neon | ~$5/mo Workers Paid + usage | Only if you specifically want the Cloudflare edge |

See **[ARCHITECTURE.md](ARCHITECTURE.md)** for the rationale.

### Required env vars

| Var | Purpose |
| --- | ------- |
| `DATABASE_URL` | Postgres connection (Neon pooled string, `?sslmode=require`). |
| `JWT_SECRET` | Signs auth session tokens. **Set a strong value** (e.g. `openssl rand -hex 32`) — it falls back to an insecure dev default if unset. Set it **once** and keep it stable; changing it invalidates everyone's sessions. |

**Signing in (no email yet):** auth is passwordless magic-link. The seeded admin
`digitallymba@gmail.com` exists on first boot; on the deployed site, enter that email and the
one-time sign-in link is shown in-app to click (mock email — swap to real email later). Invites
work the same way (copyable links).

---

## Push-to-deploy — GitHub Actions → Cloud Run (recommended)

A push to `main` builds and deploys automatically via `.github/workflows/deploy.yml`
(Cloud Build builds the Dockerfile in the cloud — no local Docker).

### One-time bootstrap

1. **Neon** — create a project, copy the **Pooled** connection string (`...-pooler...`, keep
   `?sslmode=require`).
2. **GCP** — authenticate and create the deployer service account:
   ```bash
   gcloud auth login
   bash scripts/setup-gcp.sh <PROJECT_ID>     # enables APIs, makes gh-deployer SA + gh-sa-key.json
   ```
3. **GitHub** — push the repo and set the four Actions secrets (uses the `gh` CLI):
   ```bash
   git init && git add -A && git commit -m "money-data: payments portal"
   gh repo create money-data --private --source=. --remote=origin --push
   gh secret set GCP_PROJECT_ID -b "<PROJECT_ID>"
   gh secret set GCP_SA_KEY < gh-sa-key.json
   gh secret set DATABASE_URL -b "<neon-pooled-url>"
   gh secret set JWT_SECRET   -b "$(openssl rand -hex 32)"
   rm gh-sa-key.json
   ```
   (No `gh`? Create the repo on github.com, `git remote add origin … && git push`, and add the four
   secrets under **Settings → Secrets and variables → Actions**.)

The push triggers the workflow — watch it under the repo's **Actions** tab; the deploy step prints
the `https://money-data-…-uc.a.run.app` URL.

### Ongoing
`git push` to `main` → Actions → Cloud Run redeploys. Env vars persist across revisions.

---

## Manual path — Google Cloud Run + Neon

```
 Browser ──HTTPS──▶ Cloud Run (this image: axum API + static frontend) ──sqlx──▶ Neon Postgres
                    scales 0→N · $PORT=8080                              (free, scale-to-zero)
```

### Prerequisites

| Need | Notes |
| ---- | ----- |
| **gcloud CLI**, authenticated | `gcloud auth login` ([install](https://cloud.google.com/sdk/docs/install)) |
| **A GCP project with billing enabled** | Card on file, **not charged** within free limits. Fine for B2B. |
| **A free Neon account** | [neon.tech](https://neon.tech) — no credit card. |

### 1. Create the database (Neon)

1. Sign up at **[neon.tech](https://neon.tech)** → **New Project** (pick a region near your users).
2. In **Connection Details**, copy the **Pooled connection** string. Use the **pooled**
   (`...-pooler...`) host — Cloud Run spins up many short-lived instances, and the pooler
   keeps you well under Neon's connection limit. It looks like:
   ```
   postgres://USER:PASSWORD@ep-xxx-pooler.REGION.aws.neon.tech/neondb?sslmode=require
   ```
   Keep `?sslmode=require` (the backend connects over TLS).

The app runs its own migrations and seeds demo data on first boot — no manual SQL needed.

### 2. Deploy

From the project root (`money-data/`):

```bash
gcloud config set project <PROJECT_ID>

gcloud run deploy money-data \
  --source . --region us-central1 \
  --allow-unauthenticated \
  --min-instances 0 --max-instances 3 \
  --memory 512Mi --cpu 1 --port 8080 \
  --set-env-vars "DATABASE_URL=postgres://USER:PASSWORD@ep-xxx-pooler.REGION.aws.neon.tech/neondb?sslmode=require,JWT_SECRET=$(openssl rand -hex 32)"
```

The first run prompts to enable the `run`, `cloudbuild`, and `artifactregistry` APIs — accept.
Cloud Build builds the `Dockerfile`, pushes the image to Artifact Registry, and deploys.
When it finishes it prints your URL:

```
https://money-data-xxxxxxxx-uc.a.run.app
```

### 3. Verify

```bash
curl https://<your-run-url>/api/health        # {"service":"money-data","status":"ok"}
```

Open the URL — the dark dashboard loads, KPIs animate, the ledger renders. Create a payment
link, then deploy a new revision (forces a fresh instance) and confirm the data is still
there — that's the durable-DB win.

### After deploying

**Redeploy after code changes** — rerun the deploy command. `DATABASE_URL` persists across
revisions, so you can omit `--set-env-vars` on later deploys.

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

### Hardening for a real launch (not needed for validation)

- Move the connection string out of an env var into **Secret Manager**:
  ```bash
  printf '%s' 'postgres://...-pooler.../neondb?sslmode=require' \
    | gcloud secrets create money-data-db --data-file=-
  gcloud run deploy money-data --source . --region us-central1 \
    --set-secrets DATABASE_URL=money-data-db:latest
  ```
- Add merchant authentication and switch money columns to `NUMERIC` (see the
  [ARCHITECTURE.md](ARCHITECTURE.md) roadmap).

### Cost

Cloud Run (2M req/mo, scale-to-zero) + Neon (free, no card) = **$0** at validation traffic.
Only overage risk is pennies of Artifact Registry storage for built images.

---

## Paid path — Cloudflare Containers

Runs the same image behind a thin Worker (`worker/index.ts`). This still requires a Postgres
`DATABASE_URL` (e.g. the same Neon database as above) — the app no longer has an in-memory mode.

```
Browser ──▶ Worker (worker/index.ts) ──▶ Container [ axum + static frontend ] ──▶ Neon Postgres
```

### Prerequisites

| Need | Notes |
| ---- | ----- |
| **Docker Desktop running** | Wrangler builds the image locally, then pushes it. |
| **Node 18+** | For `wrangler`. |
| **Workers _Paid_ plan (~$5/mo)** | Containers are **not** on the free tier. |

### Deploy

```bash
npm install                 # wrangler + @cloudflare/containers (root package.json)
npx wrangler login          # authorize + pick account
# Provide the database connection as a secret the container can read:
npx wrangler secret put DATABASE_URL   # paste the Neon pooled connection string
npx wrangler deploy         # builds Dockerfile (amd64), pushes, provisions container + Worker
```

Wrangler prints `https://money-data.<your-subdomain>.workers.dev`. Verify:

```bash
curl https://money-data.<your-subdomain>.workers.dev/api/health
```

**Redeploy:** `npx wrangler deploy`. **Logs:** `npx wrangler tail`. **Tear down:** `npx wrangler delete`.

> The container sleeps after idle (`sleepAfter` in `worker/index.ts`); the next request
> cold-starts it. Because data lives in Postgres now, **nothing resets** on cold start.

### Troubleshooting

| Symptom | Fix |
| ------- | --- |
| `exec format error` at runtime | Image isn't amd64. The `Dockerfile` pins `--platform=linux/amd64`. |
| Build fails immediately | Docker daemon isn't running — start Docker Desktop. |
| Backend exits on boot | `DATABASE_URL` missing/unreachable — it's required. Check the secret + Neon. |
| `wrangler` auth errors | Re-run `npx wrangler login`; confirm the Workers Paid plan. |

---

## Run the same image anywhere

The image is self-contained — any container host works, given a `DATABASE_URL`:

```bash
docker build -t money-data .
docker run -p 8080:8080 -e DATABASE_URL='postgres://...?sslmode=require' money-data
# open http://localhost:8080
```

For Fly.io: `fly launch` (detects the Dockerfile) + set `DATABASE_URL` as a secret. For
Railway/Render: point them at this repo and add the `DATABASE_URL` variable.
