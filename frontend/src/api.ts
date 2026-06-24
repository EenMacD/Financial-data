// Typed client for the money-data backend.

export type PaymentStatus = "pending" | "paid" | "settled" | "refunded";
export type RefundMethod = "card_reversal" | "direct_debit_pull";
export type Role = "admin" | "tenant";

export interface Operator {
  id: string;
  name: string;
  region: string;
  settlement_currency: string;
}

export interface Quote {
  settlement_currency: string;
  settlement_amount: number;
  presentment_currency: string;
  mid_rate: number;
  quoted_rate: number;
  converted_amount: number;
  processing_fee: number;
  total_charged: number;
  fx_markup_revenue_usd: number;
  processing_fee_revenue_usd: number;
}

export interface PaymentLink extends Quote {
  id: string;
  tenant_id: string;
  reference: string;
  trip_id: string | null;
  guest_name: string;
  description: string;
  status: PaymentStatus;
  url: string;
  ryft_session_id: string | null;
  created_at: string;
  paid_at: string | null;
}

export interface WalletView {
  currency: string;
  available: number;
  pending: number;
  available_usd: number;
  pending_usd: number;
}

export interface WalletsResponse {
  wallets: WalletView[];
  total_available_usd: number;
  total_pending_usd: number;
}

export interface Refund {
  id: string;
  payment_id: string;
  payment_reference: string;
  method: RefundMethod;
  amount: number;
  currency: string;
  status: string;
  created_at: string;
}

export interface SettlementLine {
  currency: string;
  amount: number;
  payments: number;
}

export interface SettlementBatch {
  id: string;
  created_at: string;
  lines: SettlementLine[];
  total_usd: number;
}

export interface DashboardSummary {
  operator: Operator;
  total_available_usd: number;
  total_pending_usd: number;
  fx_markup_revenue_usd: number;
  processing_revenue_usd: number;
  total_revenue_usd: number;
  processed_volume_usd: number;
  counts: {
    pending: number;
    paid: number;
    settled: number;
    refunded: number;
    total: number;
  };
}

export interface RateEntry {
  currency: string;
  usd_rate: number;
}

export interface User {
  id: string;
  email: string;
  name: string;
  role: Role;
  tenant_id: string | null;
  created_at: string;
}

export interface Tenant {
  id: string;
  name: string;
  region: string;
  settlement_currency: string;
  registration_number: string | null;
  country: string | null;
  contact_name: string | null;
  contact_email: string | null;
  ryft_subaccount_id: string | null;
  onboarding_status: string;
  onboarding_url: string | null;
  created_at: string;
}

export interface MeResponse {
  user: User;
  tenants: Tenant[];
  active_tenant_id: string;
}

export interface SessionResponse {
  token: string;
  user: User;
}

export interface MagicLinkResponse {
  sent: boolean;
  magic_link: string | null;
}

export interface InviteResponse {
  accept_link: string;
  email: string;
}

export interface SigninLinkResponse {
  email: string;
  /** Relative sign-in path; prefix with location.origin for a full URL. */
  link: string;
}

export interface SignupResponse {
  tenant: Tenant;
  onboarding_url: string | null;
}

// ---------------------------------------------------------------------------
// Session (JWT in localStorage; active tenant for the admin switcher)
// ---------------------------------------------------------------------------

const TOKEN_KEY = "md_token";
const TENANT_KEY = "md_tenant";

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}
export function setSession(token: string): void {
  localStorage.setItem(TOKEN_KEY, token);
}
export function clearSession(): void {
  localStorage.removeItem(TOKEN_KEY);
  localStorage.removeItem(TENANT_KEY);
}
export function getActiveTenant(): string | null {
  return localStorage.getItem(TENANT_KEY);
}
export function setActiveTenant(id: string): void {
  localStorage.setItem(TENANT_KEY, id);
}

export interface ApiError extends Error {
  status?: number;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...((init?.headers as Record<string, string>) ?? {}),
  };
  const token = getToken();
  if (token) headers["Authorization"] = `Bearer ${token}`;
  const tenant = getActiveTenant();
  if (tenant) headers["X-Tenant-Id"] = tenant;

  const res = await fetch(`/api${path}`, { ...init, headers });
  if (!res.ok) {
    let message = `Request failed (${res.status})`;
    try {
      const body = await res.json();
      if (body?.error) message = body.error;
    } catch {
      /* ignore */
    }
    const err: ApiError = new Error(message);
    err.status = res.status;
    throw err;
  }
  return res.json() as Promise<T>;
}

export const api = {
  // Auth
  login: (email: string, password: string) =>
    request<SessionResponse>("/auth/login", {
      method: "POST",
      body: JSON.stringify({ email, password }),
    }),
  magicLink: (email: string) =>
    request<MagicLinkResponse>("/auth/magic-link", {
      method: "POST",
      body: JSON.stringify({ email }),
    }),
  verify: (token: string) =>
    request<SessionResponse>("/auth/verify", {
      method: "POST",
      body: JSON.stringify({ token }),
    }),
  acceptInvite: (token: string, name: string) =>
    request<SessionResponse>("/invites/accept", {
      method: "POST",
      body: JSON.stringify({ token, name }),
    }),
  me: () => request<MeResponse>("/me"),

  // Tenants & users (admin portal + settings)
  listTenants: () => request<Tenant[]>("/tenants"),
  createTenant: (body: {
    name: string;
    region?: string;
    settlement_currency?: string;
    registration_number?: string;
    country?: string;
    contact_name?: string;
    contact_email?: string;
  }) => request<Tenant>("/tenants", { method: "POST", body: JSON.stringify(body) }),
  invite: (body: { email: string; role: Role; tenant_id?: string }) =>
    request<InviteResponse>("/invites", { method: "POST", body: JSON.stringify(body) }),
  tenantSigninLink: (tenantId: string, email: string) =>
    request<SigninLinkResponse>(`/tenants/${tenantId}/signin-link`, {
      method: "POST",
      body: JSON.stringify({ email }),
    }),
  listUsers: () => request<User[]>("/users"),
  createUser: (body: {
    email: string;
    name: string;
    password: string;
    role: Role;
    tenant_id?: string;
  }) => request<User>("/users", { method: "POST", body: JSON.stringify(body) }),

  // Public self-serve KYB/KYC sign-up
  signup: (body: {
    business_name: string;
    registration_number?: string;
    country?: string;
    contact_name?: string;
    contact_email?: string;
    billing_currency?: string;
  }) => request<SignupResponse>("/merchants", { method: "POST", body: JSON.stringify(body) }),

  // Dashboard data
  dashboard: () => request<DashboardSummary>("/dashboard"),
  wallets: () => request<WalletsResponse>("/wallets"),
  rates: () => request<RateEntry[]>("/rates"),
  links: () => request<PaymentLink[]>("/payment-links"),
  refunds: () => request<Refund[]>("/refunds"),
  batches: () => request<SettlementBatch[]>("/settlement/batches"),

  quote: (body: {
    settlement_currency: string;
    settlement_amount: number;
    presentment_currency: string;
  }) => request<Quote>("/quote", { method: "POST", body: JSON.stringify(body) }),

  createLink: (body: {
    guest_name: string;
    description: string;
    settlement_currency: string;
    settlement_amount: number;
    presentment_currency: string;
    trip_id?: string;
  }) =>
    request<PaymentLink>("/payment-links", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  payLink: (id: string) =>
    request<PaymentLink>(`/payment-links/${id}/pay`, { method: "POST" }),

  refund: (body: { payment_id: string; method: RefundMethod; amount?: number }) =>
    request<Refund>("/refunds", { method: "POST", body: JSON.stringify(body) }),

  settle: () => request<SettlementBatch>("/settlement/run", { method: "POST" }),
};

/// Currencies offered when billing a guest (the basic starter set).
export const BILLING_CURRENCIES = ["USD", "EUR", "GBP"];
/// All currencies the FX engine supports (settlement wallets, rates).
export const CURRENCIES = ["USD", "EUR", "GBP", "AUD", "CAD", "ZAR", "KES", "TZS"];
