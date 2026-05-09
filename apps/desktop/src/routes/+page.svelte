<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";

  // ── Types ─────────────────────────────────────────────────────────────────────

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

  function importBannerText(summaries: ImportSummary[]): string {
    const total = summaries.reduce((n, s) => n + s.transaction_count, 0);
    if (summaries.length === 1) {
      const s = summaries[0];
      return `Imported ${total} transactions from ${s.institution} ···${s.account_number_last4}`;
    }
    const institution = summaries[0].institution;
    return `Imported ${summaries.length} accounts (${total} transactions) from ${institution}`;
  }

  interface BalancePoint {
    period: string;
    closing_balance: number | null;
  }

  interface AccountSeries {
    institution: string;
    account_number_last4: string;
    account_type: string | null;
    points: BalancePoint[];
  }

  interface MonthlyFlow {
    period: string;
    income: number;
    spend: number;
  }

  interface ChartData {
    account_series: AccountSeries[];
    monthly_flows: MonthlyFlow[];
  }

  type PageState =
    | "initializing"
    | "empty"
    | "dashboard"
    | "importing"
    | "import-error";

  type ActiveView = "dashboard" | "accounts" | "transactions" | "imports" | "settings";

  // ── State ─────────────────────────────────────────────────────────────────────

  interface AppSettings {
    api_key: string | null;
    endpoint_url: string | null;
  }

  let pageState = $state<PageState>("initializing");
  let activeView = $state<ActiveView>("dashboard");
  let dashboard = $state<DashboardData | null>(null);
  let chartData = $state<ChartData | null>(null);
  let lastImport = $state<ImportSummary[] | null>(null);
  let importError = $state("");
  let dragging = $state(false);
  let importBanner = $state(false);
  let chartWidth = $state(600);

  let settingsForm = $state<{ apiKey: string; endpointUrl: string }>({ apiKey: "", endpointUrl: "" });
  let settingsSaved = $state(false);
  let settingsSaving = $state(false);

  // ── Data loading ──────────────────────────────────────────────────────────────

  async function loadDashboard() {
    try {
      const [data, chart] = await Promise.all([
        invoke<DashboardData | null>("get_dashboard"),
        invoke<ChartData | null>("get_chart_data"),
      ]);
      dashboard = data;
      chartData = chart;
      pageState = data ? "dashboard" : "empty";
    } catch {
      dashboard = null;
      chartData = null;
      pageState = "empty";
    }
  }

  async function loadSettings() {
    try {
      const s = await invoke<AppSettings>("get_settings");
      settingsForm = { apiKey: s.api_key ?? "", endpointUrl: s.endpoint_url ?? "" };
    } catch {
      // non-fatal
    }
  }

  async function saveSettings() {
    settingsSaving = true;
    try {
      await invoke("save_settings", {
        settings: {
          api_key: settingsForm.apiKey || null,
          endpoint_url: settingsForm.endpointUrl || null,
        },
      });
      settingsSaved = true;
      setTimeout(() => (settingsSaved = false), 2500);
    } finally {
      settingsSaving = false;
    }
  }

  onMount(() => {
    loadDashboard();
    loadSettings();

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
          importError =
            "Only PDF files are supported. Please drop a bank or credit card statement PDF.";
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

  // ── Drag-drop ─────────────────────────────────────────────────────────────────

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
      importError =
        "Only PDF files are supported. Please drop a bank or credit card statement PDF.";
      pageState = "import-error";
    }
  }

  async function runImport(path: string) {
    pageState = "importing";
    importError = "";
    try {
      lastImport = await invoke<ImportSummary[]>("import_statement", { path });
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

  // ── Formatting ────────────────────────────────────────────────────────────────

  function fmt(n: number) {
    return n.toLocaleString("en-US", {
      style: "currency",
      currency: "USD",
      maximumFractionDigits: 0,
    });
  }

  function fmtDate(iso: string) {
    return iso.replace("T", " ").slice(0, 16);
  }

  // ── Chart helpers ─────────────────────────────────────────────────────────────

  const TYPE_COLOR: Record<string, string> = {
    credit_card: "#e05252",
    checking: "#396cd8",
    savings: "#38a169",
  };
  const FALLBACK_COLORS = [
    "#396cd8",
    "#e05252",
    "#38a169",
    "#d69e2e",
    "#805ad5",
    "#319795",
  ];

  function seriesColor(series: AccountSeries, index: number): string {
    return series.account_type
      ? (TYPE_COLOR[series.account_type] ?? FALLBACK_COLORS[index % FALLBACK_COLORS.length])
      : FALLBACK_COLORS[index % FALLBACK_COLORS.length];
  }

  function last12Months(): string[] {
    const months: string[] = [];
    const now = new Date();
    for (let i = 11; i >= 0; i--) {
      const d = new Date(now.getFullYear(), now.getMonth() - i, 1);
      months.push(
        `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`
      );
    }
    return months;
  }

  function monthLabel(period: string): string {
    const [y, m] = period.split("-");
    const abbr = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
    return `${abbr[parseInt(m) - 1]} '${y.slice(2)}`;
  }

  const CHART_PAD = { top: 16, right: 16, bottom: 36, left: 64 };
  const CHART_H = 220;

  interface ChartGeometry {
    months: string[];
    plotW: number;
    plotH: number;
    minVal: number;
    maxVal: number;
    yTicks: number[];
    seriesPolylines: { color: string; points: string; dots: { cx: number; cy: number }[] }[];
  }

  function buildChart(cd: ChartData, totalW: number): ChartGeometry {
    const months = last12Months();
    const plotW = totalW - CHART_PAD.left - CHART_PAD.right;
    const plotH = CHART_H - CHART_PAD.top - CHART_PAD.bottom;

    // Gather all balance values to set y scale.
    const allVals: number[] = [];
    for (const s of cd.account_series) {
      for (const p of s.points) {
        if (p.closing_balance != null) allVals.push(p.closing_balance);
      }
    }

    let minVal = allVals.length ? Math.min(...allVals) : 0;
    let maxVal = allVals.length ? Math.max(...allVals) : 1000;
    if (minVal === maxVal) { minVal -= 500; maxVal += 500; }
    // Pad 10% on each side.
    const range = maxVal - minVal;
    minVal = minVal - range * 0.08;
    maxVal = maxVal + range * 0.08;

    // Nice y-ticks (5 steps).
    const yTicks = Array.from({ length: 5 }, (_, i) =>
      minVal + ((maxVal - minVal) * i) / 4
    );

    function xOf(period: string): number {
      const idx = months.indexOf(period);
      if (idx < 0) return -1;
      return months.length === 1 ? plotW / 2 : (idx / (months.length - 1)) * plotW;
    }

    function yOf(val: number): number {
      return plotH - ((val - minVal) / (maxVal - minVal)) * plotH;
    }

    const seriesPolylines = cd.account_series.map((s, i) => {
      const validPts = s.points.filter(
        (p) => months.includes(p.period) && p.closing_balance != null
      );
      const ptStr = validPts
        .map((p) => `${xOf(p.period).toFixed(1)},${yOf(p.closing_balance!).toFixed(1)}`)
        .join(" ");
      const dots = validPts.map((p) => ({
        cx: xOf(p.period),
        cy: yOf(p.closing_balance!),
      }));
      return { color: seriesColor(s, i), points: ptStr, dots };
    });

    return { months, plotW, plotH, minVal, maxVal, yTicks, seriesPolylines };
  }

  // ── Summary card computations ─────────────────────────────────────────────────

  function avg(arr: number[]): number {
    return arr.length ? arr.reduce((a, b) => a + b, 0) / arr.length : 0;
  }

  const CC_CREDIT_LIMIT = 40_000;

  function summaryCards(cd: ChartData) {
    const ccSeries = cd.account_series.filter((s) => s.account_type === "credit_card");
    const ccBalances = ccSeries.flatMap((s) =>
      s.points.filter((p) => p.closing_balance != null).map((p) => p.closing_balance!)
    );
    const avgCcBalance = avg(ccBalances);
    const avgCcUtilPct = (avgCcBalance / CC_CREDIT_LIMIT) * 100;

    const incomes = cd.monthly_flows.map((f) => f.income);
    const spends = cd.monthly_flows.map((f) => f.spend);
    const avgIncome = avg(incomes);
    const avgSpend = avg(spends);
    const avgNet = avgIncome - avgSpend;

    return { avgCcBalance, avgCcUtilPct, hasCc: ccBalances.length > 0, avgIncome, avgSpend, avgNet };
  }

  // ── Nav items ─────────────────────────────────────────────────────────────────

  const navItems: { id: ActiveView; label: string }[] = [
    { id: "dashboard", label: "Dashboard" },
    { id: "accounts", label: "Accounts" },
    { id: "transactions", label: "Transactions" },
    { id: "imports", label: "Import Log" },
    { id: "settings", label: "Settings" },
  ];
</script>

<div
  class="app-shell"
  class:dragging-overlay={dragging &&
    (pageState === "dashboard" || pageState === "importing")}
  ondragover={onDragOver}
  ondragleave={onDragLeave}
  ondrop={onDrop}
  role="region"
  aria-label="Wealth"
>
  <nav class="sidebar" aria-label="Navigation">
    {#each navItems as item}
      <button
        class="nav-btn"
        class:active={activeView === item.id}
        class:nav-bottom={item.id === "settings"}
        onclick={() => (activeView = item.id)}
        title={item.label}
        aria-label={item.label}
        aria-current={activeView === item.id ? "page" : undefined}
      >
        {#if item.id === "dashboard"}
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <rect x="3" y="3" width="7" height="7" rx="1" />
            <rect x="14" y="3" width="7" height="7" rx="1" />
            <rect x="3" y="14" width="7" height="7" rx="1" />
            <rect x="14" y="14" width="7" height="7" rx="1" />
          </svg>
        {:else if item.id === "accounts"}
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <line x1="3" y1="22" x2="21" y2="22" />
            <line x1="6" y1="18" x2="6" y2="11" />
            <line x1="10" y1="18" x2="10" y2="11" />
            <line x1="14" y1="18" x2="14" y2="11" />
            <line x1="18" y1="18" x2="18" y2="11" />
            <polygon points="12 2 20 7 4 7" />
          </svg>
        {:else if item.id === "transactions"}
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <line x1="8" y1="6" x2="21" y2="6" />
            <line x1="8" y1="12" x2="21" y2="12" />
            <line x1="8" y1="18" x2="21" y2="18" />
            <circle cx="3" cy="6" r="1" fill="currentColor" stroke="none" />
            <circle cx="3" cy="12" r="1" fill="currentColor" stroke="none" />
            <circle cx="3" cy="18" r="1" fill="currentColor" stroke="none" />
          </svg>
        {:else if item.id === "imports"}
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <polyline points="17 8 12 3 7 8" />
            <line x1="12" y1="3" x2="12" y2="15" />
          </svg>
        {:else if item.id === "settings"}
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
        {/if}
      </button>
    {/each}
  </nav>

  <main class="content">
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
            <path
              d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"
            />
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
          {importBannerText(lastImport)}
        </div>
      {/if}

      {#if pageState === "importing"}
        <div class="import-overlay">
          <div class="spinner" aria-label="Importing…"></div>
          <p class="hint">Extracting transactions…</p>
        </div>
      {/if}

      {#if dashboard}
        {#if activeView === "dashboard"}
          <!-- 12-month balance chart -->
          {#if chartData && chartData.account_series.length > 0}
            {@const geo = buildChart(chartData, chartWidth)}
            {@const cards = summaryCards(chartData)}
            <section class="chart-section" aria-label="12-month balance history">
              <h2 class="section-title">Balance History</h2>
              <div
                class="chart-wrap"
                bind:clientWidth={chartWidth}
              >
                <svg
                  width={chartWidth}
                  height={CHART_H}
                  role="img"
                  aria-label="Account balance chart"
                >
                  <g transform="translate({CHART_PAD.left},{CHART_PAD.top})">
                    <!-- Y grid lines + labels -->
                    {#each geo.yTicks as tick}
                      {@const y = geo.plotH - ((tick - geo.minVal) / (geo.maxVal - geo.minVal)) * geo.plotH}
                      <line
                        x1="0" y1={y}
                        x2={geo.plotW} y2={y}
                        class="grid-line"
                      />
                      <text x="-8" y={y} class="axis-label y-label">{fmt(tick)}</text>
                    {/each}

                    <!-- X axis labels -->
                    {#each geo.months as m, i}
                      {@const x = geo.months.length === 1 ? geo.plotW / 2 : (i / (geo.months.length - 1)) * geo.plotW}
                      <text
                        x={x}
                        y={geo.plotH + 24}
                        class="axis-label x-label"
                        class:x-label-show={i % 2 === 0 || i === geo.months.length - 1}
                      >{monthLabel(m)}</text>
                    {/each}

                    <!-- Series lines -->
                    {#each geo.seriesPolylines as s}
                      {#if s.points}
                        <polyline
                          points={s.points}
                          fill="none"
                          stroke={s.color}
                          stroke-width="2"
                          stroke-linejoin="round"
                          stroke-linecap="round"
                        />
                        {#each s.dots as d}
                          <circle cx={d.cx} cy={d.cy} r="3.5" fill={s.color} />
                        {/each}
                      {/if}
                    {/each}
                  </g>
                </svg>
              </div>

              <!-- Legend -->
              <div class="legend">
                {#each chartData.account_series as s, i}
                  <span class="legend-item">
                    <span class="legend-dot" style="background:{seriesColor(s, i)}"></span>
                    {s.institution} ···{s.account_number_last4}
                    {#if s.account_type}
                      <span class="legend-type">({s.account_type.replace("_", " ")})</span>
                    {/if}
                  </span>
                {/each}
              </div>
            </section>

            <!-- Summary cards -->
            <section class="cards-row" aria-label="Monthly averages">
              {#if cards.hasCc}
                <div class="card">
                  <p class="card-label">Avg CC Utilization</p>
                  <p class="card-value">{fmt(cards.avgCcBalance)}</p>
                  <p class="card-sub">{cards.avgCcUtilPct.toFixed(1)}% of {fmt(CC_CREDIT_LIMIT)} limit</p>
                </div>
              {/if}
              <div class="card">
                <p class="card-label">Avg Monthly Income</p>
                <p class="card-value">{fmt(cards.avgIncome)}</p>
                <p class="card-sub">credits per month</p>
              </div>
              <div class="card">
                <p class="card-label">Avg Monthly Spend</p>
                <p class="card-value">{fmt(cards.avgSpend)}</p>
                <p class="card-sub">debits per month</p>
              </div>
              <div class="card" class:net-positive={cards.avgNet >= 0} class:net-negative={cards.avgNet < 0}>
                <p class="card-label">Avg Monthly Net</p>
                <p class="card-value">{fmt(Math.abs(cards.avgNet))}</p>
                <p class="card-sub">{cards.avgNet >= 0 ? "surplus" : "deficit"} per month</p>
              </div>
            </section>
          {:else}
            <div class="chart-section">
              <p class="hint">No data in the last 12 months. Import a statement to get started.</p>
            </div>
          {/if}

        {:else if activeView === "accounts"}
          <section class="cards-row" aria-label="Account balances">
            {#if dashboard.account_balances.length === 0}
              <p class="hint">No accounts yet. Import a statement to get started.</p>
            {:else}
              {#each dashboard.account_balances as acct}
                <div class="card">
                  <p class="card-label">
                    {acct.institution} ···{acct.account_number_last4}
                  </p>
                  <p class="card-value">
                    {acct.closing_balance != null
                      ? fmt(acct.closing_balance)
                      : "–"}
                  </p>
                  <p class="card-sub">closing balance · {acct.statement_period}</p>
                </div>
              {/each}
            {/if}
          </section>
        {:else if activeView === "transactions"}
          <div class="center-frame">
            <div class="state-card">
              <svg class="icon" viewBox="0 0 24 24" aria-hidden="true">
                <line x1="8" y1="6" x2="21" y2="6" />
                <line x1="8" y1="12" x2="21" y2="12" />
                <line x1="8" y1="18" x2="21" y2="18" />
                <circle cx="3" cy="6" r="1" fill="currentColor" stroke="none" />
                <circle cx="3" cy="12" r="1" fill="currentColor" stroke="none" />
                <circle cx="3" cy="18" r="1" fill="currentColor" stroke="none" />
              </svg>
              <p class="headline">Transaction History</p>
              <p class="hint">Coming soon</p>
            </div>
          </div>
        {:else if activeView === "imports"}
          <section class="recent-section" aria-label="Import log">
            <h2 class="section-title">Import Log</h2>
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
    {/if}

    {#if activeView === "settings"}
      <section class="settings-section" aria-label="Settings">
        <h2 class="section-title">Settings</h2>
        <form class="settings-form" onsubmit={(e) => { e.preventDefault(); saveSettings(); }}>
          <div class="field-group">
            <label for="api-key" class="field-label">API Key</label>
            <input
              id="api-key"
              type="password"
              class="field-input"
              placeholder="sk-ant-…"
              bind:value={settingsForm.apiKey}
              autocomplete="off"
            />
            <p class="field-hint">Stored locally in app data. Falls back to the <code>ANTHROPIC_API_KEY</code> environment variable if blank.</p>
          </div>
          <div class="field-group">
            <label for="endpoint-url" class="field-label">Endpoint URL</label>
            <input
              id="endpoint-url"
              type="text"
              class="field-input"
              placeholder="https://api.anthropic.com/v1/messages"
              bind:value={settingsForm.endpointUrl}
            />
            <p class="field-hint">Leave blank to use the default Anthropic API endpoint.</p>
          </div>
          <div class="settings-actions">
            <button type="submit" disabled={settingsSaving}>
              {settingsSaving ? "Saving…" : "Save"}
            </button>
            {#if settingsSaved}
              <span class="settings-saved">Saved</span>
            {/if}
          </div>
        </form>
      </section>
    {/if}
  </main>
</div>

<style>
  :global(html), :global(body) {
    margin: 0;
    padding: 0;
    overflow: hidden;
  }

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

  /* ── Layout ── */

  .app-shell {
    height: 100vh;
    overflow: hidden;
    display: flex;
    flex-direction: row;
    position: relative;
    box-sizing: border-box;
  }

  .app-shell.dragging-overlay::after {
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

  /* ── Sidebar ── */

  .sidebar {
    width: 52px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 0.75rem 0;
    gap: 0.25rem;
    background: #ebebeb;
    border-right: 1px solid #ddd;
    height: 100%;
  }

  @media (prefers-color-scheme: dark) {
    .sidebar {
      background: #111;
      border-right-color: #222;
    }
  }

  .nav-btn {
    all: unset;
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 8px;
    color: #888;
    cursor: pointer;
    transition:
      background 0.15s,
      color 0.15s;
  }

  .nav-btn:hover {
    background: rgba(0, 0, 0, 0.07);
    color: #444;
  }

  @media (prefers-color-scheme: dark) {
    .nav-btn:hover {
      background: rgba(255, 255, 255, 0.08);
      color: #ccc;
    }
  }

  .nav-btn.active {
    color: #396cd8;
    background: rgba(57, 108, 216, 0.1);
  }

  .nav-btn svg {
    width: 18px;
    height: 18px;
    stroke: currentColor;
    fill: none;
    stroke-width: 1.75;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  /* ── Content ── */

  .content {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 2rem;
    box-sizing: border-box;
    position: relative;
    overflow-y: auto;
  }

  .center-frame {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 60vh;
  }

  /* ── Drop zone ── */

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
    transition:
      border-color 0.2s,
      background 0.2s;
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

  /* ── Spinner ── */

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

  /* ── Cards ── */

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

  .cards-row {
    width: 100%;
    max-width: 900px;
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
    margin-bottom: 2rem;
  }

  .card {
    flex: 1 1 160px;
    background: #fff;
    border: 1px solid #e2e8f0;
    border-radius: 10px;
    padding: 1rem 1.25rem;
  }

  @media (prefers-color-scheme: dark) {
    .card {
      background: #2d2d2d;
      border-color: #3a3a3a;
    }
  }

  .card.net-positive {
    border-color: rgba(56, 161, 105, 0.4);
  }

  .card.net-negative {
    border-color: rgba(229, 62, 62, 0.4);
  }

  .card-label {
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #666;
    margin: 0 0 0.3rem;
  }

  .card-value {
    font-size: 1.5rem;
    font-weight: 700;
    margin: 0 0 0.2rem;
    font-variant-numeric: tabular-nums;
  }

  .card-sub {
    font-size: 0.75rem;
    color: #888;
    margin: 0;
  }

  @media (prefers-color-scheme: dark) {
    .card-label,
    .card-sub { color: #aaa; }
  }

  /* ── Chart ── */

  .chart-section {
    width: 100%;
    max-width: 900px;
    margin-bottom: 1.5rem;
  }

  .chart-wrap {
    width: 100%;
    overflow: hidden;
    border-radius: 8px;
  }

  .grid-line {
    stroke: #e2e8f0;
    stroke-width: 1;
  }

  @media (prefers-color-scheme: dark) {
    .grid-line { stroke: #2d3748; }
  }

  .axis-label {
    font-size: 11px;
    fill: #888;
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
  }

  .y-label {
    text-anchor: end;
    dominant-baseline: middle;
  }

  .x-label {
    text-anchor: middle;
    display: none;
  }

  .x-label-show {
    display: block;
  }

  /* ── Legend ── */

  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem 1.5rem;
    margin-top: 0.75rem;
    font-size: 0.82rem;
  }

  .legend-item {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .legend-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .legend-type {
    color: #888;
    font-size: 0.78rem;
  }

  /* ── Sections ── */

  .recent-section {
    width: 100%;
    max-width: 900px;
  }

  .section-title {
    font-size: 0.9rem;
    font-weight: 600;
    margin: 0 0 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #666;
  }

  @media (prefers-color-scheme: dark) {
    .section-title { color: #aaa; }
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
    .imports-table td { border-bottom-color: #2d3748; }
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

  /* ── Text ── */

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

  /* ── Icons ── */

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

  /* ── Settings ── */

  .nav-bottom {
    margin-top: auto;
  }

  .settings-section {
    width: 100%;
    max-width: 540px;
  }

  .settings-form {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
  }

  .field-group {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .field-label {
    font-size: 0.82rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #555;
  }

  @media (prefers-color-scheme: dark) {
    .field-label { color: #aaa; }
  }

  .field-input {
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
    font-size: 0.9rem;
    padding: 0.5rem 0.75rem;
    border: 1px solid #ccc;
    border-radius: 6px;
    background: #fff;
    color: inherit;
    outline: none;
    transition: border-color 0.15s;
  }

  .field-input:focus {
    border-color: #396cd8;
  }

  @media (prefers-color-scheme: dark) {
    .field-input {
      background: #2d2d2d;
      border-color: #444;
      color: #f6f6f6;
    }
  }

  .field-hint {
    font-size: 0.78rem;
    color: #888;
    margin: 0;
  }

  .field-hint code {
    font-family: monospace;
    background: rgba(0, 0, 0, 0.06);
    padding: 0.1em 0.3em;
    border-radius: 3px;
  }

  @media (prefers-color-scheme: dark) {
    .field-hint { color: #aaa; }
    .field-hint code { background: rgba(255,255,255,0.08); }
  }

  .settings-actions {
    display: flex;
    align-items: center;
    gap: 1rem;
  }

  .settings-saved {
    font-size: 0.85rem;
    color: #38a169;
    font-weight: 500;
  }

  /* ── Button ── */

  button:not(.nav-btn) {
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

  button:not(.nav-btn):hover {
    background-color: #2d5bc7;
  }
</style>
