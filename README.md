# money·data

A B2B **payment orchestration platform** for African safari & leisure operators.
It sits between **Ebury** (FX + multi-currency wallets) and **Ecommpay** (card
payment gateway):

- Operators generate **currency-converted payment links** for international guests.
- The platform earns revenue through a **2% FX markup** and a **1% processing fee**.
- Captured funds settle into **operator-specific currency wallets** via a single
  **daily batch transfer**.
- A **dark-mode merchant dashboard** shows live wallet balances, a payments ledger,
  a revenue breakdown, and a **dual-method refund system** (card reversal _or_
  direct debit pull).

> MVP scope: data is persisted in **Postgres** (free Neon in production), seeded with
> demo data on first boot; the Ebury/Ecommpay integrations are still mocked. No merchant
> auth yet.

## Stack

| Layer    | Tech                                                        |
| -------- | ----------------------------------------------------------- |
| Backend  | Rust · [axum](https://github.com/tokio-rs/axum) · tokio     |
| Data     | Postgres via [sqlx](https://github.com/launchbadge/sqlx) (Neon free tier) |
| Frontend | Vite · TypeScript · [GSAP](https://gsap.com) animations     |

## Run it

Two terminals:

```bash
# 1) Backend  → http://127.0.0.1:8080  (needs a Postgres DATABASE_URL)
#    Throwaway DB:  docker run -d --name md-pg -p 5432:5432 -e POSTGRES_PASSWORD=pw postgres:16-alpine
cd backend
DATABASE_URL=postgres://postgres:pw@127.0.0.1:5432/postgres cargo run
#    (migrations run + demo data seeds automatically on first boot)

# 2) Frontend → http://localhost:5173  (proxies /api to the backend)
cd frontend
npm install
npm run dev
```

Open **http://localhost:5173**.

## Deploy

Everything (API + frontend) builds into one Docker image. The recommended **free** host is
**Google Cloud Run + Neon Postgres** ($0 at validation traffic); Cloudflare Containers is a
paid alternative. See **[DEPLOY.md](DEPLOY.md)** for both. Quick local container run:

```bash
docker build -t money-data .
docker run -p 8080:8080 -e DATABASE_URL=postgres://...?sslmode=require money-data  # http://localhost:8080
```

## How the money math works

For an operator who wants to receive `amount` in their settlement currency,
charging a guest in a different presentment currency:

```
quoted_rate    = mid_rate × (1 + 2% FX markup)
converted      = amount × quoted_rate          # in presentment currency
processing_fee = converted × 1%
guest pays     = converted + processing_fee
```

Platform revenue = the FX markup spread + the processing fee (reported in USD).
When a guest pays, funds land in the wallet's **pending** balance; the daily
settlement batch sweeps **pending → available**.

## API

| Method | Path                          | Purpose                                  |
| ------ | ----------------------------- | ---------------------------------------- |
| GET    | `/api/dashboard`              | KPI summary (balances, revenue, counts)  |
| GET    | `/api/wallets`                | Per-currency wallet balances             |
| GET    | `/api/rates`                  | Mid-market FX rates (Ebury mock)         |
| POST   | `/api/quote`                  | Live conversion preview                  |
| GET    | `/api/payment-links`          | Payments ledger                          |
| POST   | `/api/payment-links`          | Create a currency-converted link         |
| POST   | `/api/payment-links/:id/pay`  | Simulate the guest paying (Ecommpay)     |
| POST   | `/api/refunds`                | Refund (`card_reversal` / `direct_debit_pull`) |
| GET    | `/api/refunds`                | Refund history                           |
| POST   | `/api/settlement/run`         | Run the daily batch sweep                |
| GET    | `/api/settlement/batches`     | Settlement batch history                 |

## Project layout

```
money-data/
├── backend/            Rust API (axum)
│   └── src/
│       ├── main.rs     router + server bootstrap
│       ├── models.rs   domain types
│       ├── db.rs       Postgres pool, migrations, demo seed
│       ├── fx.rs       Ebury FX mock + quote math
│       └── handlers.rs HTTP handlers (Postgres-backed)
└── frontend/           Vite + TypeScript dashboard
    └── src/
        ├── main.ts     render + interaction logic
        ├── api.ts      typed API client
        ├── anim.ts     GSAP motion helpers
        ├── format.ts   currency/number/date formatting
        └── style.css   dark-mode theme
```
