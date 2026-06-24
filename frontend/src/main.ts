import "./style.css";
import {
  api,
  BILLING_CURRENCIES,
  clearSession,
  getActiveTenant,
  getToken,
  setActiveTenant,
  setSession,
  type ApiError,
  type DashboardSummary,
  type PaymentLink,
  type Quote,
  type Refund,
  type RefundMethod,
  type SettlementBatch,
  type Tenant,
  type User,
  type WalletsResponse,
} from "./api";
import { money, usd, num, timeAgo } from "./format";
import { revealStagger, countUp, pulse, toast, openModal } from "./anim";

interface AppData {
  dashboard: DashboardSummary;
  wallets: WalletsResponse;
  links: PaymentLink[];
  refunds: Refund[];
  batches: SettlementBatch[];
  rates: { currency: string; usd_rate: number }[];
  users: User[];
}

interface Session {
  user: User;
  tenants: Tenant[];
  activeTenantId: string;
}

const app = document.querySelector<HTMLDivElement>("#app")!;
let session: Session | null = null;
let data: AppData | null = null;
let currentTab: Tab = "incoming";

type Tab = "incoming" | "refunds" | "treasury" | "tenants" | "settings";
const TABS: { id: Tab; label: string; icon: string }[] = [
  { id: "incoming", label: "Incoming payments", icon: "↘" },
  { id: "refunds", label: "Refunds", icon: "↺" },
  { id: "treasury", label: "Treasury", icon: "▤" },
  { id: "tenants", label: "Tenants", icon: "▦" },
  { id: "settings", label: "Settings", icon: "⚙" },
];

// Operator-facing labels for the underlying engine states.
const STATUS_LABEL: Record<string, string> = {
  pending: "Pending",
  paid: "Processed",
  settled: "Delivered",
  refunded: "Refunded",
};

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => {
    return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]!;
  });
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

function parseHash(): { path: string; params: URLSearchParams } {
  const raw = location.hash.replace(/^#/, "");
  const [path, query] = raw.split("?");
  return { path: path || "/", params: new URLSearchParams(query || "") };
}

async function route(): Promise<void> {
  const { path, params } = parseHash();
  try {
    if (path === "/signup") return renderSignup();
    if (path === "/login" || path === "/") {
      if (path === "/" && getToken()) {
        location.hash = "#/incoming";
        return;
      }
      return renderLogin();
    }
    if (path === "/auth/verify") return await handleVerify(params.get("token"));
    if (path === "/accept-invite") return renderAcceptInvite(params.get("token"));

    // Everything below requires a session.
    if (!getToken()) {
      location.hash = "#/login";
      return;
    }
    const tab = path.replace(/^\//, "") as Tab;
    currentTab = TABS.some((t) => t.id === tab) ? tab : "incoming";
    await loadAndRender(true);
  } catch (e) {
    handleError(e);
  }
}

function handleError(e: unknown): void {
  const err = e as ApiError;
  if (err?.status === 401) {
    clearSession();
    session = null;
    location.hash = "#/login";
    return;
  }
  toast(err?.message || "Something went wrong", "error");
}

// ---------------------------------------------------------------------------
// Data loading
// ---------------------------------------------------------------------------

async function loadSession(): Promise<Session> {
  const me = await api.me();
  if (!getActiveTenant() && me.active_tenant_id) setActiveTenant(me.active_tenant_id);
  return {
    user: me.user,
    tenants: me.tenants,
    activeTenantId: getActiveTenant() || me.active_tenant_id,
  };
}

async function loadAll(): Promise<AppData> {
  const [dashboard, wallets, links, refunds, batches, rates, users] = await Promise.all([
    api.dashboard(),
    api.wallets(),
    api.links(),
    api.refunds(),
    api.batches(),
    api.rates(),
    api.listUsers().catch(() => [] as User[]),
  ]);
  return { dashboard, wallets, links, refunds, batches, rates, users };
}

async function loadAndRender(animate = false): Promise<void> {
  if (!session) session = await loadSession();
  data = await loadAll();
  renderApp();
  if (animate) {
    revealStagger(".kpi");
    revealStagger(".card", 0.1);
  }
  animateCounters();
}

async function refresh(): Promise<void> {
  data = await loadAll();
  renderApp();
  animateCounters();
}

// ---------------------------------------------------------------------------
// App shell (sidebar + topbar + active tab)
// ---------------------------------------------------------------------------

function renderApp(): void {
  const d = data!;
  const s = session!;
  const op = d.dashboard.operator;
  const isAdmin = s.user.role === "admin";

  // Tenants is an admin-only portal; bounce non-admins who land on it directly.
  if (currentTab === "tenants" && !isAdmin) {
    location.hash = "#/incoming";
    return;
  }

  const nav = TABS.filter((t) => t.id !== "tenants" || isAdmin)
    .map(
      (t) => `
      <a class="nav__item ${t.id === currentTab ? "is-active" : ""}" href="#/${t.id}">
        <span class="nav__icon">${t.icon}</span>${t.label}
      </a>`
    )
    .join("");

  app.innerHTML = `
    <div class="layout">
      <aside class="sidebar">
        <div class="brand">
          <span class="brand__mark">◈</span>
          <span>money<b>·</b>data</span>
        </div>
        <nav class="nav">${nav}</nav>
        <div class="sidebar__foot">
          <div class="who">
            <b>${esc(s.user.name)}</b>
            <span>${esc(s.user.email)} · ${s.user.role}</span>
          </div>
          <button class="btn btn--ghost btn--sm" data-action="signout">Sign out</button>
        </div>
      </aside>

      <div class="main">
        <header class="topbar">
          <div class="op-chip">
            <b>${esc(op.name)}</b>
            <span>${esc(op.region)} · bills in ${op.settlement_currency}</span>
          </div>
          <div class="topbar__spacer"></div>
          <button class="btn btn--primary" data-action="open-create">＋ Create payment</button>
        </header>
        <div class="shell">${renderTab(d)}</div>
      </div>
    </div>`;
}

function renderTab(d: AppData): string {
  switch (currentTab) {
    case "incoming":
      return renderIncoming(d);
    case "refunds":
      return renderRefundsTab(d);
    case "treasury":
      return renderTreasury(d);
    case "tenants":
      return renderTenants();
    case "settings":
      return renderSettings(d);
  }
}

// ---------------------------------------------------------------------------
// Tab: Incoming payments
// ---------------------------------------------------------------------------

function renderKpis(d: AppData): string {
  const c = d.dashboard;
  const kpi = (label: string, val: number, accent: string, sub: string) => `
    <div class="kpi" style="--accent:${accent}">
      <div class="kpi__label">${label}</div>
      <div class="kpi__value mono" data-count="${val}">$0</div>
      <div class="kpi__sub">${sub}</div>
    </div>`;
  return `
    <section class="kpis">
      ${kpi("Available balance", c.total_available_usd, "var(--primary)", "delivered across all wallets")}
      ${kpi("Pending settlement", c.total_pending_usd, "var(--amber)", `${c.counts.paid} processed awaiting payout`)}
      ${kpi("Platform revenue", c.total_revenue_usd, "var(--info)", "FX markup + processing fees")}
      ${kpi("Processed volume", c.processed_volume_usd, "#c084fc", `${c.counts.total} payments`)}
    </section>`;
}

function statusChip(status: string): string {
  return `<span class="chip chip--${status}">${STATUS_LABEL[status] ?? status}</span>`;
}

function renderIncoming(d: AppData): string {
  const rows = d.links
    .map((l) => {
      const actions =
        l.status === "pending"
          ? `<button class="btn btn--sm" data-action="pay" data-id="${l.id}">Simulate payment</button>`
          : l.status === "paid" || l.status === "settled"
          ? `<button class="btn btn--sm btn--danger" data-action="refund" data-id="${l.id}">Refund</button>`
          : `<span class="cell-muted">—</span>`;
      return `
        <tr>
          <td class="mono cell-strong">${l.reference}</td>
          <td class="mono">${l.trip_id ? esc(l.trip_id) : `<span class="cell-muted">—</span>`}</td>
          <td>
            <div class="cell-strong">${esc(l.guest_name)}</div>
            ${l.description ? `<div class="cell-muted" style="font-size:12px">${esc(l.description)}</div>` : ""}
          </td>
          <td>
            <div class="cell-strong">${money(l.total_charged, l.presentment_currency)}</div>
            <div class="cell-muted" style="font-size:12px"><span class="ccy-badge">${l.presentment_currency}</span></div>
          </td>
          <td>${statusChip(l.status)}</td>
          <td class="cell-muted">${timeAgo(l.created_at)}</td>
          <td class="text-right">${actions}</td>
        </tr>`;
    })
    .join("");

  return `
    ${renderKpis(d)}
    <section class="card">
      <div class="card__head">
        <h2>Incoming payments</h2>
        <div class="spacer"></div>
        <span class="cell-muted" style="font-size:12px">${d.links.length} entries</span>
      </div>
      <table>
        <thead>
          <tr><th>Reference</th><th>Trip ID</th><th>Payer</th><th>Amount</th><th>Status</th><th>Created</th><th></th></tr>
        </thead>
        <tbody>${
          rows || `<tr><td colspan="7"><div class="empty">No payments yet — create one from the top right.</div></td></tr>`
        }</tbody>
      </table>
    </section>`;
}

// ---------------------------------------------------------------------------
// Tab: Refunds
// ---------------------------------------------------------------------------

function methodLabel(m: RefundMethod): string {
  return m === "card_reversal" ? "Card reversal (Ryft)" : "Direct debit pull (Ebury)";
}

function renderRefundsTab(d: AppData): string {
  const body = d.refunds.length
    ? `<table>
        <thead><tr><th>Reference</th><th>Method</th><th>Amount</th><th>When</th></tr></thead>
        <tbody>${d.refunds
          .map(
            (r) => `<tr>
              <td class="mono">${r.payment_reference}</td>
              <td>${methodLabel(r.method)}</td>
              <td class="cell-strong">${money(r.amount, r.currency)}</td>
              <td class="cell-muted">${timeAgo(r.created_at)}</td>
            </tr>`
          )
          .join("")}</tbody>
      </table>`
    : `<div class="empty">No refunds issued. Refund a processed or delivered payment from the Incoming tab.</div>`;
  return `
    <section class="card">
      <div class="card__head"><h2>Refunds</h2></div>
      ${d.refunds.length ? body : `<div class="card__body">${body}</div>`}
    </section>`;
}

// ---------------------------------------------------------------------------
// Tab: Treasury
// ---------------------------------------------------------------------------

function renderTreasury(d: AppData): string {
  const totals = d.wallets.wallets
    .map((w) => {
      const total = w.available + w.pending;
      return `
        <div class="ccy-total">
          <div class="ccy-total__head">
            <span class="ccy-badge">${w.currency}</span>
            <span class="cell-muted">≈ ${usd(w.available_usd + w.pending_usd)}</span>
          </div>
          <div class="ccy-total__amt mono">${money(total, w.currency)}</div>
          <div class="cell-muted" style="font-size:12px">received in ${w.currency}</div>
        </div>`;
    })
    .join("");

  return `
    <section class="card">
      <div class="card__head">
        <h2>Treasury · total received per currency</h2>
        <div class="spacer"></div>
        <button class="btn btn--primary btn--sm" data-action="settle">⟳ Run daily settlement</button>
      </div>
      <div class="card__body"><div class="ccy-totals">${totals || `<div class="empty">No balances yet.</div>`}</div></div>
    </section>
    <div class="grid">
      <div class="col">${renderWallets(d)}</div>
      <div class="col">${renderRevenue(d)}${renderRates(d)}</div>
    </div>`;
}

function renderWallets(d: AppData): string {
  const rows = d.wallets.wallets
    .map((w) => {
      const total = w.available + w.pending;
      const availPct = total > 0 ? (w.available / total) * 100 : 0;
      const pendPct = total > 0 ? (w.pending / total) * 100 : 0;
      return `
        <div class="wallet">
          <div class="wallet__top">
            <span class="wallet__ccy">${w.currency}</span>
            <span class="wallet__usd">≈ ${usd(w.available_usd + w.pending_usd)}</span>
          </div>
          <div class="wallet__amt mono">${money(w.available, w.currency)}</div>
          <div class="bar">
            <div class="bar__avail" style="width:${availPct}%"></div>
            <div class="bar__pend" style="width:${pendPct}%"></div>
          </div>
          <div class="wallet__legend">
            <span><span class="dot" style="background:var(--primary)"></span>Available ${money(w.available, w.currency)}</span>
            <span><span class="dot" style="background:var(--amber)"></span>Pending ${money(w.pending, w.currency)}</span>
          </div>
        </div>`;
    })
    .join("");
  return `
    <section class="card">
      <div class="card__head"><h2>Wallet balances</h2></div>
      <div class="card__body" style="padding-top:4px;padding-bottom:6px">${rows}</div>
    </section>`;
}

function renderRevenue(d: AppData): string {
  const c = d.dashboard;
  const total = c.total_revenue_usd || 1;
  const fxPct = (c.fx_markup_revenue_usd / total) * 100;
  const procPct = (c.processing_revenue_usd / total) * 100;
  return `
    <section class="card">
      <div class="card__head"><h2>Revenue breakdown</h2></div>
      <div class="card__body">
        <div class="rev-split">
          <div class="rev-split__fx" style="width:${fxPct}%"></div>
          <div class="rev-split__proc" style="width:${procPct}%"></div>
        </div>
        <div class="rev-line"><span><span class="dot" style="background:var(--amber)"></span>FX markup (2%)</span><b>${usd(
          c.fx_markup_revenue_usd
        )}</b></div>
        <div class="rev-line"><span><span class="dot" style="background:var(--primary)"></span>Processing fee (1%)</span><b>${usd(
          c.processing_revenue_usd
        )}</b></div>
        <div class="rev-line"><span class="cell-strong">Total earned</span><b style="color:var(--primary)">${usd(
          c.total_revenue_usd
        )}</b></div>
      </div>
    </section>`;
}

function renderRates(d: AppData): string {
  const items = d.rates
    .map(
      (r) =>
        `<div class="rate"><span>USD → ${r.currency}</span><b class="mono">${num(
          r.usd_rate,
          r.usd_rate < 10 ? 4 : 2
        )}</b></div>`
    )
    .join("");
  return `
    <section class="card">
      <div class="card__head"><h2>Live FX rates · Ebury</h2></div>
      <div class="card__body"><div class="rates">${items}</div></div>
    </section>`;
}

// ---------------------------------------------------------------------------
// Tab: Settings (team users + — for admins — tenants/onboarding)
// ---------------------------------------------------------------------------

function renderSettings(d: AppData): string {
  const s = session!;
  const isAdmin = s.user.role === "admin";

  const userRows = d.users.length
    ? d.users
        .map(
          (u) => `<tr>
            <td class="cell-strong">${esc(u.name)}</td>
            <td class="cell-muted">${esc(u.email)}</td>
            <td><span class="chip chip--paid">${u.role}</span></td>
          </tr>`
        )
        .join("")
    : `<tr><td colspan="3"><div class="empty">No team members yet.</div></td></tr>`;

  const usersCard = `
    <section class="card">
      <div class="card__head"><h2>Team members</h2></div>
      <div class="card__body">
        <form id="add-user-form" class="inline-form">
          <input class="input" name="name" placeholder="Full name" required />
          <input class="input" name="email" type="email" placeholder="teammate@business.com" required />
          <input class="input" name="password" type="password" placeholder="Temp password (min 8)" minlength="8" required />
          ${
            isAdmin
              ? `<select class="select" name="role">
                   <option value="tenant">Tenant user</option>
                   <option value="admin">Admin</option>
                 </select>`
              : `<input type="hidden" name="role" value="tenant" />`
          }
          <button class="btn btn--primary" type="submit">Add user</button>
        </form>
        <p class="cell-muted" style="font-size:12px;margin:8px 0 0">They sign in with this email and password.</p>
        <table style="margin-top:8px">
          <thead><tr><th>Name</th><th>Email</th><th>Role</th></tr></thead>
          <tbody>${userRows}</tbody>
        </table>
      </div>
    </section>`;

  return usersCard;
}

// ---------------------------------------------------------------------------
// Tab: Tenants (admin-only portal) — the single home for tenant management
// and sending one-time sign-in links.
// ---------------------------------------------------------------------------

function renderTenants(): string {
  const s = session!;
  const tRows = s.tenants
    .map((t) => {
      const isActive = t.id === s.activeTenantId;
      return `<tr>
        <td>
          <div class="cell-strong">${esc(t.name)}${
        isActive ? ` <span class="chip chip--paid">Active</span>` : ""
      }</div>
          <div class="cell-muted" style="font-size:12px">${t.country ? esc(t.country) : esc(t.region) || "—"}${
        t.ryft_subaccount_id ? ` · ${esc(t.ryft_subaccount_id)}` : ""
      }</div>
        </td>
        <td>${t.settlement_currency}</td>
        <td><span class="chip chip--${onboardingChip(t.onboarding_status)}">${esc(t.onboarding_status)}</span></td>
        <td class="text-right">
          <button class="btn btn--sm" data-action="select-tenant" data-id="${t.id}"${
        isActive ? " disabled" : ""
      }>${isActive ? "Selected" : "Select"}</button>
          <button class="btn btn--sm btn--primary" data-action="invite-tenant" data-id="${t.id}" data-name="${esc(
        t.name
      )}">Send sign-in link</button>
        </td>
      </tr>`;
    })
    .join("");
  return `
    <section class="card">
      <div class="card__head"><h2>Tenants · onboarded businesses</h2></div>
      <div class="card__body">
        <form id="add-tenant-form" class="inline-form">
          <input class="input" name="name" placeholder="Business name" required />
          <input class="input" name="country" placeholder="Country" />
          <select class="select" name="billing_currency">
            ${BILLING_CURRENCIES.map((c) => `<option value="${c}">${c}</option>`).join("")}
          </select>
          <button class="btn btn--primary" type="submit">Add tenant</button>
        </form>
        <p class="cell-muted" style="font-size:12px;margin:8px 0 0">
          Select a tenant to view its data, or send a one-time sign-in link to its team.
        </p>
        <table style="margin-top:8px">
          <thead><tr><th>Business</th><th>Bills in</th><th>Onboarding</th><th></th></tr></thead>
          <tbody>${tRows || `<tr><td colspan="4"><div class="empty">No tenants yet.</div></td></tr>`}</tbody>
        </table>
      </div>
    </section>`;
}

function onboardingChip(status: string): string {
  if (status === "verified") return "settled";
  if (status === "rejected") return "refunded";
  if (status === "in_review") return "paid";
  return "pending";
}

// ---------------------------------------------------------------------------
// Create payment (modal)
// ---------------------------------------------------------------------------

function renderQuote(q: Quote | null, error?: string): string {
  if (error)
    return `<div class="quote"><div class="quote__row" style="color:var(--danger)">${esc(error)}</div></div>`;
  if (!q) return `<div class="quote"><div class="quote__row">Enter an amount to preview the charge…</div></div>`;
  return `
    <div class="quote">
      <div class="quote__row"><span>Amount</span><b>${money(q.converted_amount, q.presentment_currency)}</b></div>
      <div class="quote__row"><span>Processing fee (1%)</span><b>${money(
        q.processing_fee,
        q.presentment_currency
      )}</b></div>
      <div class="quote__row quote__total"><span>Payer is charged</span><b>${money(
        q.total_charged,
        q.presentment_currency
      )}</b></div>
      <div class="quote__row quote__rev"><span>Your revenue</span><b class="quote__rev">${usd(
        q.fx_markup_revenue_usd + q.processing_fee_revenue_usd
      )}</b></div>
    </div>`;
}

function openCreatePaymentModal(): void {
  const el = document.createElement("div");
  el.innerHTML = `
    <div class="modal__head">
      <h3>Create payment</h3>
      <p>Generate a Ryft hosted link to bill a payer for a trip.</p>
    </div>
    <div class="modal__body">
      <form id="create-form">
        <div class="row2">
          <div class="field">
            <label>Trip ID</label>
            <input class="input mono" name="trip_id" placeholder="e.g. TRIP-MM-2048" />
          </div>
          <div class="field">
            <label>Bill in</label>
            <select class="select" name="currency">
              ${BILLING_CURRENCIES.map((c) => `<option value="${c}">${c}</option>`).join("")}
            </select>
          </div>
        </div>
        <div class="row2">
          <div class="field">
            <label>Payer name</label>
            <input class="input" name="guest_name" placeholder="e.g. Emma Thompson" required />
          </div>
          <div class="field">
            <label>Amount</label>
            <input class="input mono" name="amount" type="number" min="1" step="0.01" value="2500" required />
          </div>
        </div>
        <div class="field">
          <label>Payer email <span class="cell-muted">(optional — to email the link)</span></label>
          <input class="input" name="email" type="email" placeholder="payer@email.com" />
        </div>
        <div id="quote-preview"></div>
        <button class="btn btn--primary" type="submit" style="width:100%;justify-content:center">Generate payment link</button>
      </form>
      <div id="create-result"></div>
    </div>`;

  const close = openModal(el);
  const form = el.querySelector<HTMLFormElement>("#create-form")!;
  const preview = el.querySelector<HTMLDivElement>("#quote-preview")!;
  const result = el.querySelector<HTMLDivElement>("#create-result")!;

  let timer: number | undefined;
  const updatePreview = () => {
    const fd = new FormData(form);
    const amount = Number(fd.get("amount"));
    const ccy = String(fd.get("currency"));
    if (!amount || amount <= 0) {
      preview.innerHTML = renderQuote(null);
      return;
    }
    window.clearTimeout(timer);
    timer = window.setTimeout(async () => {
      try {
        const q = await api.quote({
          settlement_currency: ccy,
          settlement_amount: amount,
          presentment_currency: ccy,
        });
        preview.innerHTML = renderQuote(q);
      } catch (e) {
        preview.innerHTML = renderQuote(null, (e as Error).message);
      }
    }, 180);
  };
  form.addEventListener("input", updatePreview);
  updatePreview();

  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const fd = new FormData(form);
    const ccy = String(fd.get("currency"));
    const tripId = String(fd.get("trip_id") || "").trim();
    const email = String(fd.get("email") || "").trim();
    try {
      const link = await api.createLink({
        guest_name: String(fd.get("guest_name") || ""),
        description: "",
        settlement_currency: ccy,
        presentment_currency: ccy,
        settlement_amount: Number(fd.get("amount")),
        trip_id: tripId || undefined,
      });
      const mailto = `mailto:${encodeURIComponent(email)}?subject=${encodeURIComponent(
        "Your payment link"
      )}&body=${encodeURIComponent(`Please complete your payment here:\n${link.url}`)}`;
      form.style.display = "none";
      preview.style.display = "none";
      result.innerHTML = `
        <div class="success-note">Payment link <b>${link.reference}</b> created.</div>
        <div class="linkbox">
          <input readonly value="${link.url}" />
          <button class="btn btn--sm" data-action="copy" data-url="${link.url}">Copy</button>
        </div>
        <div class="modal__actions">
          <a class="btn btn--ghost" href="${mailto}">Email link</a>
          <button class="btn btn--primary" id="create-done">Done</button>
        </div>`;
      result.querySelector("#create-done")!.addEventListener("click", async () => {
        close();
        await refresh();
      });
      toast(`Payment link ${link.reference} created`, "success");
    } catch (err) {
      toast((err as Error).message, "error");
    }
  });
}

// ---------------------------------------------------------------------------
// Refund modal
// ---------------------------------------------------------------------------

function openRefundModal(id: string): void {
  const link = data?.links.find((l) => l.id === id);
  if (!link) return;
  let method: RefundMethod = "card_reversal";

  const el = document.createElement("div");
  el.innerHTML = `
    <div class="modal__head">
      <h3>Refund ${link.reference}</h3>
      <p>${esc(link.guest_name)} · ${money(link.settlement_amount, link.settlement_currency)}</p>
    </div>
    <div class="modal__body">
      <div class="method-toggle">
        <button type="button" class="method" data-method="card_reversal" aria-pressed="true">
          <b>Card reversal</b><span>Reverse the original charge via Ryft</span>
        </button>
        <button type="button" class="method" data-method="direct_debit_pull" aria-pressed="false">
          <b>Direct debit pull</b><span>Pull funds from the wallet via Ebury</span>
        </button>
      </div>
      <div class="field">
        <label>Refund amount (${link.settlement_currency})</label>
        <input class="input mono" id="refund-amount" type="number" min="0.01" step="0.01"
          max="${link.settlement_amount}" value="${link.settlement_amount}" />
      </div>
      <div class="modal__actions">
        <button class="btn btn--ghost" id="refund-cancel">Cancel</button>
        <button class="btn btn--primary" id="refund-confirm">Confirm refund</button>
      </div>
    </div>`;

  const close = openModal(el);

  el.querySelectorAll<HTMLButtonElement>(".method").forEach((btn) => {
    btn.addEventListener("click", () => {
      method = btn.dataset.method as RefundMethod;
      el.querySelectorAll<HTMLButtonElement>(".method").forEach((b) =>
        b.setAttribute("aria-pressed", String(b === btn))
      );
    });
  });

  el.querySelector("#refund-cancel")!.addEventListener("click", () => close());
  el.querySelector("#refund-confirm")!.addEventListener("click", async () => {
    const amount = Number((el.querySelector("#refund-amount") as HTMLInputElement).value);
    try {
      const r = await api.refund({ payment_id: id, method, amount });
      close();
      toast(`Refunded ${money(r.amount, r.currency)} via ${methodLabel(r.method)}`, "success");
      await refresh();
    } catch (e) {
      toast((e as Error).message, "error");
    }
  });
}

// ---------------------------------------------------------------------------
// Auth screens
// ---------------------------------------------------------------------------

function authShell(inner: string): string {
  return `
    <div class="auth">
      <div class="auth__card card">
        <div class="brand brand--center">
          <span class="brand__mark">◈</span>
          <span>money<b>·</b>data</span>
        </div>
        ${inner}
      </div>
    </div>`;
}

function renderLogin(): void {
  app.innerHTML = authShell(`
    <h1 class="auth__title">Sign in</h1>
    <p class="auth__sub">Enter your email and password.</p>
    <form id="login-form">
      <div class="field">
        <label>Work email</label>
        <input class="input" name="email" type="email" placeholder="you@business.com" required autofocus />
      </div>
      <div class="field">
        <label>Password</label>
        <input class="input" name="password" type="password" placeholder="••••••••" required />
      </div>
      <button class="btn btn--primary" type="submit" style="width:100%;justify-content:center">Sign in</button>
    </form>
    <div id="login-result"></div>
    <p class="auth__alt">New travel business? <a href="#/signup">Sign up with KYB/KYC →</a></p>
  `);
}

function renderMessage(title: string, body: string): void {
  app.innerHTML = authShell(`<h1 class="auth__title">${title}</h1><p class="auth__sub">${body}</p>`);
}

// Require an explicit click before consuming the single-use token: emailed links
// are pre-fetched by inbox security scanners, which would otherwise burn the token
// before the real user clicks (leaving them with an "expired" link).
function handleVerify(token: string | null): void {
  if (!token) {
    location.hash = "#/login";
    return;
  }
  app.innerHTML = authShell(`
    <h1 class="auth__title">Confirm sign in</h1>
    <p class="auth__sub">Click below to finish signing in to money·data.</p>
    <button class="btn btn--primary" id="verify-btn" style="width:100%;justify-content:center">Complete sign in</button>
  `);
  const btn = document.querySelector<HTMLButtonElement>("#verify-btn")!;
  btn.addEventListener("click", () => void completeVerify(token, btn));
}

async function completeVerify(token: string, btn: HTMLButtonElement): Promise<void> {
  btn.disabled = true;
  renderMessage("Signing you in…", "One moment.");
  try {
    const s = await api.verify(token);
    setSession(s.token);
    session = null;
    if (s.user.tenant_id) setActiveTenant(s.user.tenant_id);
    location.hash = "#/incoming";
  } catch {
    app.innerHTML = authShell(`
      <h1 class="auth__title">Link expired</h1>
      <p class="auth__sub">That sign-in link is invalid or has already been used.</p>
      <a class="btn btn--primary" href="#/login" style="width:100%;justify-content:center">Back to sign in</a>
    `);
  }
}

function renderAcceptInvite(token: string | null): void {
  if (!token) {
    location.hash = "#/login";
    return;
  }
  app.innerHTML = authShell(`
    <h1 class="auth__title">Accept your invite</h1>
    <p class="auth__sub">Choose a display name to finish setting up your access.</p>
    <form id="accept-form" data-token="${esc(token)}">
      <div class="field">
        <label>Your name</label>
        <input class="input" name="name" placeholder="e.g. Amara Otieno" required autofocus />
      </div>
      <button class="btn btn--primary" type="submit" style="width:100%;justify-content:center">Accept &amp; sign in</button>
    </form>
  `);
}

function renderSignup(): void {
  app.innerHTML = authShell(`
    <h1 class="auth__title">List your travel business</h1>
    <p class="auth__sub">Tell us about your business to begin Ryft KYB/KYC onboarding.</p>
    <form id="signup-form">
      <div class="field">
        <label>Business name</label>
        <input class="input" name="business_name" placeholder="e.g. Savannah Trails Safari Co." required />
      </div>
      <div class="row2">
        <div class="field">
          <label>Company registration no.</label>
          <input class="input mono" name="registration_number" placeholder="e.g. CRP-2024-00123" />
        </div>
        <div class="field">
          <label>Country</label>
          <input class="input" name="country" placeholder="e.g. Kenya" />
        </div>
      </div>
      <div class="row2">
        <div class="field">
          <label>Contact name</label>
          <input class="input" name="contact_name" placeholder="Full name" />
        </div>
        <div class="field">
          <label>Bill in</label>
          <select class="select" name="billing_currency">
            ${BILLING_CURRENCIES.map((c) => `<option value="${c}">${c}</option>`).join("")}
          </select>
        </div>
      </div>
      <div class="field">
        <label>Contact email</label>
        <input class="input" name="contact_email" type="email" placeholder="ops@business.com" />
      </div>
      <p class="cell-muted" style="font-size:12px;margin:0 0 12px">
        You'll complete KYB document checks (registration, ownership, ID) on the secure Ryft page after submitting.
      </p>
      <button class="btn btn--primary" type="submit" style="width:100%;justify-content:center">Start onboarding</button>
    </form>
    <div id="signup-result"></div>
    <p class="auth__alt">Already onboarded? <a href="#/login">Sign in →</a></p>
  `);
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

async function handleLogin(form: HTMLFormElement): Promise<void> {
  const fd = new FormData(form);
  const email = String(fd.get("email") || "").trim();
  const password = String(fd.get("password") || "");
  const result = document.querySelector<HTMLDivElement>("#login-result")!;
  try {
    const s = await api.login(email, password);
    setSession(s.token);
    session = null;
    if (s.user.tenant_id) setActiveTenant(s.user.tenant_id);
    location.hash = "#/incoming";
  } catch (e) {
    result.innerHTML = `<div class="error-note">${esc((e as Error).message)}</div>`;
  }
}

async function handleAcceptInvite(form: HTMLFormElement): Promise<void> {
  const token = form.dataset.token!;
  const name = String(new FormData(form).get("name") || "").trim();
  try {
    const s = await api.acceptInvite(token, name);
    setSession(s.token);
    session = null;
    if (s.user.tenant_id) setActiveTenant(s.user.tenant_id);
    location.hash = "#/incoming";
  } catch (e) {
    toast((e as Error).message, "error");
  }
}

async function handleSignup(form: HTMLFormElement): Promise<void> {
  const fd = new FormData(form);
  const result = document.querySelector<HTMLDivElement>("#signup-result")!;
  try {
    const res = await api.signup({
      business_name: String(fd.get("business_name") || ""),
      registration_number: String(fd.get("registration_number") || "") || undefined,
      country: String(fd.get("country") || "") || undefined,
      contact_name: String(fd.get("contact_name") || "") || undefined,
      contact_email: String(fd.get("contact_email") || "") || undefined,
      billing_currency: String(fd.get("billing_currency") || "USD"),
    });
    form.style.display = "none";
    result.innerHTML = `
      <div class="success-note">${esc(res.tenant.name)} created — status <b>${esc(
      res.tenant.onboarding_status
    )}</b>.</div>
      ${
        res.onboarding_url
          ? `<a class="btn btn--primary" href="${res.onboarding_url}" target="_blank" rel="noopener" style="width:100%;justify-content:center">Continue to verification →</a>`
          : ""
      }
      <p class="auth__alt" style="margin-top:14px"><a href="#/login">Back to sign in</a></p>`;
  } catch (e) {
    toast((e as Error).message, "error");
  }
}

async function handleCreateUser(form: HTMLFormElement): Promise<void> {
  const fd = new FormData(form);
  const name = String(fd.get("name") || "").trim();
  const email = String(fd.get("email") || "").trim();
  const password = String(fd.get("password") || "");
  const role = String(fd.get("role") || "tenant") as "admin" | "tenant";
  try {
    await api.createUser({ email, name, password, role });
    form.reset();
    toast(`Added ${email}`, "success");
    await loadAndRender();
  } catch (e) {
    toast((e as Error).message, "error");
  }
}

async function handleInviteTenant(tenantId: string, name: string): Promise<void> {
  const el = document.createElement("div");
  el.innerHTML = `
    <div class="modal__head">
      <h3>Send sign-in link · ${esc(name)}</h3>
      <p>Generate a one-time sign-in link for this tenant, then copy and send it.</p>
    </div>
    <div class="modal__body">
      <form id="invite-tenant-form">
        <div class="field">
          <label>Email</label>
          <input class="input" name="email" type="email" placeholder="owner@business.com" required autofocus />
        </div>
        <button class="btn btn--primary" type="submit" style="width:100%;justify-content:center">Generate sign-in link</button>
      </form>
      <div id="invite-tenant-result"></div>
    </div>`;
  const close = openModal(el);
  const form = el.querySelector<HTMLFormElement>("#invite-tenant-form")!;
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const email = String(new FormData(form).get("email") || "").trim();
    try {
      const res = await api.tenantSigninLink(tenantId, email);
      const url = `${location.origin}/${res.link}`;
      el.querySelector<HTMLDivElement>("#invite-tenant-result")!.innerHTML = `
        <div class="success-note">Sign-in link for ${esc(res.email)} (copy &amp; send):</div>
        <div class="linkbox">
          <input readonly value="${url}" />
          <button class="btn btn--sm" data-action="copy" data-url="${url}">Copy</button>
        </div>
        <div class="modal__actions"><button class="btn btn--primary" id="invite-tenant-done">Done</button></div>`;
      el.querySelector("#invite-tenant-done")!.addEventListener("click", () => close());
    } catch (err) {
      toast((err as Error).message, "error");
    }
  });
}

async function handleAddTenant(form: HTMLFormElement): Promise<void> {
  const fd = new FormData(form);
  try {
    await api.createTenant({
      name: String(fd.get("name") || ""),
      country: String(fd.get("country") || "") || undefined,
      region: String(fd.get("country") || "") || undefined,
      settlement_currency: String(fd.get("billing_currency") || "USD"),
    });
    toast("Tenant added", "success");
    session = null; // refresh tenant list
    await loadAndRender();
  } catch (e) {
    toast((e as Error).message, "error");
  }
}

async function handlePay(id: string): Promise<void> {
  try {
    const link = await api.payLink(id);
    toast(`${link.guest_name} paid ${money(link.total_charged, link.presentment_currency)}`, "success");
    await refresh();
  } catch (e) {
    toast((e as Error).message, "error");
  }
}

async function handleSettle(): Promise<void> {
  try {
    const batch = await api.settle();
    toast(`Settled ${usd(batch.total_usd)} into wallets (${batch.id})`, "success");
    await refresh();
  } catch (e) {
    toast((e as Error).message, "error");
  }
}

// ---------------------------------------------------------------------------
// Animations
// ---------------------------------------------------------------------------

function animateCounters(): void {
  document.querySelectorAll<HTMLElement>("[data-count]").forEach((el) => {
    const to = Number(el.dataset.count);
    countUp(el, to, (v) => usd(v));
  });
}

// ---------------------------------------------------------------------------
// Event delegation
// ---------------------------------------------------------------------------

document.addEventListener("click", (e) => {
  const btn = (e.target as HTMLElement).closest<HTMLElement>("[data-action]");
  if (!btn) return;
  const action = btn.dataset.action!;
  if (action === "copy") {
    e.preventDefault();
    navigator.clipboard?.writeText(btn.dataset.url!);
    toast("Copied to clipboard", "info");
    return;
  }
  pulse(btn);
  if (action === "open-create") openCreatePaymentModal();
  else if (action === "settle") void handleSettle();
  else if (action === "pay") void handlePay(btn.dataset.id!);
  else if (action === "refund") openRefundModal(btn.dataset.id!);
  else if (action === "invite-tenant") void handleInviteTenant(btn.dataset.id!, btn.dataset.name || "");
  else if (action === "select-tenant") {
    setActiveTenant(btn.dataset.id!);
    if (session) session.activeTenantId = btn.dataset.id!;
    location.hash = "#/incoming"; // view the selected tenant's data
  } else if (action === "signout") {
    clearSession();
    session = null;
    location.hash = "#/login";
  }
});

document.addEventListener("submit", (e) => {
  const form = e.target as HTMLFormElement;
  const handlers: Record<string, (f: HTMLFormElement) => void> = {
    "login-form": (f) => void handleLogin(f),
    "accept-form": (f) => void handleAcceptInvite(f),
    "signup-form": (f) => void handleSignup(f),
    "add-user-form": (f) => void handleCreateUser(f),
    "add-tenant-form": (f) => void handleAddTenant(f),
  };
  const handler = handlers[form.id];
  if (handler) {
    e.preventDefault();
    handler(form);
  }
});

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

window.addEventListener("hashchange", () => void route());

(async function boot() {
  if (!location.hash) location.hash = getToken() ? "#/incoming" : "#/login";
  await route();
})();
