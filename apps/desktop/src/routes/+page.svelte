<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";

  interface AccountBalance {
    institution: string;
    account_number_last4: string;
    closing_balance: number | null;
    statement_period: string;
  }

  interface RecentImport {
    institution: string;
    account_number_last4: string;
    statement_period: string;
    transaction_count: number;
    imported_at: string;
  }

  interface DashboardData {
    spend_30d: number;
    spend_90d: number;
    account_balances: AccountBalance[];
    recent_imports: RecentImport[];
  }

  interface ImportSummary {
    institution: string;
    account_number_last4: string;
    statement_period: string;
    transaction_count: number;
  }

  type PageState = "initializing" | "empty" | "dashboard" | "importing" | "import-error";

  let pageState = $state<PageState>("initializing");
  let dashboard = $state<DashboardData | null>(null);
  let lastImport = $state<ImportSummary | null>(null);
  let importError = $state("");
  let dragging = $state(false);
  let importBanner = $state(false);

  async function loadDashboard() {
    const data = await invoke<DashboardData | null>("get_dashboard");
    dashboard = data;
    pageState = data ? "dashboard" : "empty";
  }

  onMount(() => {
    loadDashboard();

    const win = getCurrentWindow();
    const unlistenPromise = win.onDragDropEvent((event) => {
      if (event.payload.type === "enter") {
        dragging = true;
      } else if (event.payload.type === "leave") {
        dragging = false;
      } else if (event.payload.type === "drop") {
        dragging = false;
        const paths: string[] = event.payload.paths;
        const pdf = paths.find((p) => p.toLowerCase().endsWith(".pdf"));
        if (!pdf) {
          importError = "Only PDF files are supported. Please drop a bank or credit card statement PDF.";
          pageState = "import-error";
          return;
        }
        runImport(pdf);
      }
    });
    return () => {
      unlistenPromise.then((f) => f());
    };
  });

  function onDragOver(e: DragEvent) {
    e.preventDefault();
    dragging = true;
  }

  function onDragLeave(e: DragEvent) {
    if (!(e.currentTarget as Element).contains(e.relatedTarget as Node)) {
      dragging = false;
    }
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    const file = e.dataTransfer?.files[0];
    if (file && !file.name.toLowerCase().endsWith(".pdf")) {
      importError = "Only PDF files are supported. Please drop a bank or credit card statement PDF.";
      pageState = "import-error";
    }
  }

  async function runImport(path: string) {
    pageState = "importing";
    importError = "";
    try {
      lastImport = await invoke<ImportSummary>("import_statement", { path });
      await loadDashboard();
      importBanner = true;
      setTimeout(() => (importBanner = false), 4000);
    } catch (e) {
      importError = String(e);
      pageState = "import-error";
    }
  }

  function dismissError() {
    pageState = dashboard ? "dashboard" : "empty";
    importError = "";
  }

  function fmt(n: number) {
    return n.toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 2 });
  }

  function fmtDate(iso: string) {
    return iso.replace("T", " ").slice(0, 16);
  }
</script>

<main
  class="container"
  class:dragging-overlay={dragging && pageState === "dashboard"}
  ondragover={onDragOver}
  ondragleave={onDragLeave}
  ondrop={onDrop}
  role="region"
  aria-label="Wealth dashboard"
>
  {#if pageState === "initializing"}
    <div class="center-frame">
      <div class="spinner" aria-label="Loading…"></div>
    </div>

  {:else if pageState === "empty" || (pageState === "importing" && !dashboard)}
    <div
      class="drop-zone"
      class:dragging
      class:loading={pageState === "importing"}
    >
      {#if pageState === "importing"}
        <div class="spinner" aria-label="Importing…"></div>
        <p class="hint">Extracting transactions…</p>
      {:else}
        <svg class="icon" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
          <polyline points="14 2 14 8 20 8" />
          <line x1="12" y1="12" x2="12" y2="18" />
          <line x1="9" y1="15" x2="15" y2="15" />
        </svg>
        <p class="headline">Drop a statement PDF here</p>
        <p class="hint">Bank and credit card statements supported</p>
      {/if}
    </div>

  {:else if pageState === "import-error"}
    <div class="center-frame">
      <div class="state-card error-card">
        <svg class="icon error-icon" viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="12" cy="12" r="10" />
          <line x1="12" y1="8" x2="12" y2="12" />
          <line x1="12" y1="16" x2="12.01" y2="16" />
        </svg>
        <p class="headline">Import failed</p>
        <p class="hint">{importError}</p>
        <button onclick={dismissError}>Go back</button>
      </div>
    </div>

  {:else if pageState === "dashboard" || pageState === "importing"}
    {#if importBanner && lastImport}
      <div class="import-banner">
        <svg class="icon-sm success-icon" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
          <polyline points="22 4 12 14.01 9 11.01" />
        </svg>
        Imported {lastImport.transaction_count} transactions from {lastImport.institution} ···{lastImport.account_number_last4}
      </div>
    {/if}

    <header class="page-header">
      <h1 class="page-title">Dashboard</h1>
      <p class="page-hint">Drop a PDF anywhere to import a statement</p>
    </header>

    {#if pageState === "importing"}
      <div class="import-overlay">
        <div class="spinner" aria-label="Importing…"></div>
        <p class="hint">Extracting transactions…</p>
      </div>
    {/if}

    {#if dashboard}
      <section class="cards-row" aria-label="Spending summary">
        <div class="card">
          <p class="card-label">Last 30 days</p>
          <p class="card-value">{fmt(dashboard.spend_30d)}</p>
          <p class="card-sub">debit spend</p>
        </div>
        <div class="card">
          <p class="card-label">Last 90 days</p>
          <p class="card-value">{fmt(dashboard.spend_90d)}</p>
          <p class="card-sub">debit spend</p>
        </div>
        {#each dashboard.account_balances as acct}
          <div class="card">
            <p class="card-label">{acct.institution} ···{acct.account_number_last4}</p>
            <p class="card-value">{acct.closing_balance != null ? fmt(acct.closing_balance) : "–"}</p>
            <p class="card-sub">closing balance · {acct.statement_period}</p>
          </div>
        {/each}
      </section>

      <section class="recent-section" aria-label="Recent imports">
        <h2 class="section-title">Recent Imports</h2>
        {#if dashboard.recent_imports.length === 0}
          <p class="hint">No imports yet.</p>
        {:else}
          <table class="imports-table">
            <thead>
              <tr>
                <th>Account</th>
                <th>Period</th>
                <th class="num-col">Transactions</th>
                <th>Imported</th>
              </tr>
            </thead>
            <tbody>
              {#each dashboard.recent_imports as imp}
                <tr>
                  <td>{imp.institution} ···{imp.account_number_last4}</td>
                  <td>{imp.statement_period}</td>
                  <td class="num-col">{imp.transaction_count}</td>
                  <td>{fmtDate(imp.imported_at)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </section>
    {/if}
  {/if}
</main>

<style>
  :root {
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
    font-size: 16px;
    line-height: 24px;
    font-weight: 400;
    color: #0f0f0f;
    background-color: #f6f6f6;
    -webkit-font-smoothing: antialiased;
  }

  @media (prefers-color-scheme: dark) {
    :root {
      color: #f6f6f6;
      background-color: #1a1a1a;
    }
  }

  .container {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 2rem;
    box-sizing: border-box;
    position: relative;
  }

  .container.dragging-overlay::after {
    content: "Drop PDF to import";
    position: fixed;
    inset: 0;
    background: rgba(57, 108, 216, 0.12);
    border: 3px dashed #396cd8;
    border-radius: 12px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 1.4rem;
    font-weight: 600;
    color: #396cd8;
    pointer-events: none;
    z-index: 100;
  }

  .center-frame {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 60vh;
  }

  .drop-zone {
    width: 100%;
    max-width: 560px;
    margin-top: 10vh;
    border: 2px dashed #ccc;
    border-radius: 12px;
    padding: 3rem 2rem;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.75rem;
    transition: border-color 0.2s, background 0.2s;
  }

  .drop-zone.dragging {
    border-color: #396cd8;
    background: rgba(57, 108, 216, 0.06);
  }

  .drop-zone.loading {
    border-style: solid;
    border-color: #396cd8;
    cursor: wait;
  }

  .import-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.35);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 1rem;
    z-index: 50;
    color: #fff;
  }

  .spinner {
    width: 40px;
    height: 40px;
    border: 3px solid rgba(57, 108, 216, 0.2);
    border-top-color: #396cd8;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .state-card {
    width: 100%;
    max-width: 560px;
    border-radius: 12px;
    padding: 2.5rem 2rem;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.75rem;
  }

  .error-card {
    border: 1px solid #e53e3e;
    background: rgba(229, 62, 62, 0.05);
  }

  .import-banner {
    width: 100%;
    max-width: 900px;
    background: rgba(56, 161, 105, 0.12);
    border: 1px solid #38a169;
    border-radius: 8px;
    padding: 0.6rem 1rem;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.9rem;
    color: #276749;
    margin-bottom: 1rem;
  }

  @media (prefers-color-scheme: dark) {
    .import-banner { color: #68d391; }
  }

  .page-header {
    width: 100%;
    max-width: 900px;
    margin-bottom: 1.5rem;
  }

  .page-title {
    font-size: 1.6rem;
    font-weight: 700;
    margin: 0 0 0.2rem;
  }

  .page-hint {
    font-size: 0.85rem;
    color: #888;
    margin: 0;
  }

  .cards-row {
    width: 100%;
    max-width: 900px;
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
    margin-bottom: 2rem;
  }

  .card {
    flex: 1 1 180px;
    background: #fff;
    border: 1px solid #e2e8f0;
    border-radius: 10px;
    padding: 1.1rem 1.25rem;
  }

  @media (prefers-color-scheme: dark) {
    .card {
      background: #2d2d2d;
      border-color: #3a3a3a;
    }
  }

  .card-label {
    font-size: 0.78rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #666;
    margin: 0 0 0.35rem;
  }

  .card-value {
    font-size: 1.6rem;
    font-weight: 700;
    margin: 0 0 0.2rem;
    font-variant-numeric: tabular-nums;
  }

  .card-sub {
    font-size: 0.78rem;
    color: #888;
    margin: 0;
  }

  @media (prefers-color-scheme: dark) {
    .card-label, .card-sub { color: #aaa; }
  }

  .recent-section {
    width: 100%;
    max-width: 900px;
  }

  .section-title {
    font-size: 1rem;
    font-weight: 600;
    margin: 0 0 0.75rem;
  }

  .imports-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.9rem;
  }

  .imports-table th,
  .imports-table td {
    padding: 0.55rem 0.75rem;
    text-align: left;
    border-bottom: 1px solid #e2e8f0;
  }

  @media (prefers-color-scheme: dark) {
    .imports-table th,
    .imports-table td {
      border-bottom-color: #2d3748;
    }
  }

  .imports-table th {
    font-weight: 600;
    color: #555;
    font-size: 0.78rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  @media (prefers-color-scheme: dark) {
    .imports-table th { color: #aaa; }
  }

  .num-col {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .headline {
    font-size: 1.1rem;
    font-weight: 600;
    margin: 0;
  }

  .hint {
    font-size: 0.9rem;
    color: #666;
    margin: 0;
  }

  @media (prefers-color-scheme: dark) {
    .hint { color: #aaa; }
  }

  .icon {
    width: 48px;
    height: 48px;
    stroke: currentColor;
    fill: none;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
    opacity: 0.5;
  }

  .icon-sm {
    width: 18px;
    height: 18px;
    stroke: currentColor;
    fill: none;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
    flex-shrink: 0;
  }

  .error-icon { color: #e53e3e; opacity: 1; }
  .success-icon { color: #38a169; opacity: 1; }

  button {
    border-radius: 8px;
    border: 1px solid transparent;
    padding: 0.5em 1.2em;
    font-size: 0.9em;
    font-weight: 500;
    background-color: #396cd8;
    color: #fff;
    cursor: pointer;
    transition: background 0.2s;
  }

  button:hover { background-color: #2d5bc7; }
</style>
