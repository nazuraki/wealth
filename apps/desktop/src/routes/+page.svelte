<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick, untrack } from "svelte";
  import {
    Chart,
    LineController,
    LineElement,
    PointElement,
    LinearScale,
    CategoryScale,
    Tooltip,
    Legend,
  } from "chart.js";

  Chart.register(LineController, LineElement, PointElement, LinearScale, CategoryScale, Tooltip, Legend);

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
    display_name: string | null;
    color: string | null;
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

  interface Account {
    id: number;
    institution: string;
    account_number_last4: string;
    account_type: string | null;
    display_name: string | null;
    color: string | null;
    closing_balance: number | null;
    statement_period: string | null;
  }

  type PageState =
    | "initializing"
    | "empty"
    | "dashboard"
    | "importing"
    | "import-error";

  type ActiveView = "dashboard" | "accounts" | "transactions" | "imports" | "settings";

  interface AppSettings {
    api_key: string | null;
    endpoint_url: string | null;
  }

  interface Transaction {
    id: number;
    date: string;
    description: string;
    category: string;
    amount: number;
    kind: string;
    account_id: number;
    institution: string;
    account_number_last4: string;
  }

  interface TransactionPage {
    rows: Transaction[];
    total: number;
  }

  // ── State ─────────────────────────────────────────────────────────────────────

  let pageState = $state<PageState>("initializing");
  let activeView = $state<ActiveView>("dashboard");
  let dashboard = $state<DashboardData | null>(null);
  let chartData = $state<ChartData | null>(null);
  let lastImport = $state<ImportSummary[] | null>(null);
  let importError = $state("");
  let dragging = $state(false);
  let importBanner = $state(false);

  let chartFrom = $state("");
  let chartTo = $state("");
  let availablePeriods = $state<string[]>([]);

  let accounts = $state<Account[]>([]);
  let editingAccountId = $state<number | null>(null);
  let editingName = $state("");

  const TX_PAGE_SIZE = 100;
  const TX_MAX_LOADED = 1000;

  let txFilterAccount = $state("");
  let txFilterDateFrom = $state("");
  let txFilterDateTo = $state("");
  let txFilterCategory = $state("");
  let txFilterKinds = $state<string[]>(["debit", "credit"]);

  let txOffset = $state(0);
  let txTotal = $state(0);
  let txLoadedRows = $state<Transaction[]>([]);
  let txLoading = $state(false);
  let txRequestId = 0;

  let contentEl = $state<HTMLElement | null>(null);

  function focusOnMount(node: HTMLElement) {
    node.focus();
    if (node instanceof HTMLInputElement) node.select();
  }

  let settingsForm = $state<{ apiKey: string; endpointUrl: string }>({ apiKey: "", endpointUrl: "" });
  let settingsSaved = $state(false);
  let settingsSaving = $state(false);

  // ── Chart canvas / instance ───────────────────────────────────────────────────

  let canvasEl = $state<HTMLCanvasElement | null>(null);
  let chartInstance: Chart | null = null;

  $effect(() => {
    if (chartFrom && chartTo) {
      console.log("[chart] fetching get_chart_data", { from: chartFrom, to: chartTo });
      invoke<ChartData | null>("get_chart_data", { from: chartFrom, to: chartTo })
        .then(data => {
          console.log("[chart] got data, monthly_flows:", data?.monthly_flows?.length ?? "null");
          chartData = data;
        })
        .catch((e) => { console.error("[chart] get_chart_data failed:", e); });
    }
  });

  $effect(() => {
    if (!canvasEl || !chartData) return;

    const months = chartData
      ? monthsBetween(chartFrom, chartTo)
      : [];

    const datasets = chartData.account_series.map((s, i) => {
      const color = s.color ?? seriesColorFallback(s, i);
      const data = months.map(m => {
        const pt = s.points.find(p => p.period === m);
        return pt?.closing_balance ?? null;
      });
      return {
        label: accountLabel(s),
        data,
        borderColor: color,
        backgroundColor: color,
        pointBackgroundColor: color,
        pointRadius: 4,
        pointHoverRadius: 6,
        tension: 0,
        spanGaps: false,
      };
    });

    if (chartInstance) {
      chartInstance.data.labels = months.map(xMonthLabel);
      chartInstance.data.datasets = datasets;
      chartInstance.update();
    } else {
      chartInstance = new Chart(canvasEl, {
        type: "line",
        data: { labels: months.map(xMonthLabel), datasets },
        options: {
          responsive: true,
          maintainAspectRatio: false,
          interaction: { mode: "index", intersect: false },
          plugins: {
            legend: {
              position: "bottom",
              labels: { boxWidth: 12, padding: 16, font: { size: 12 } },
              onClick(e, item, legend) {
                // Default Chart.js toggle behavior
                const ci = legend.chart;
                const meta = ci.getDatasetMeta(item.datasetIndex!);
                meta.hidden = !meta.hidden;
                ci.update();
              },
            },
            tooltip: {
              callbacks: {
                label(ctx) {
                  if (ctx.parsed.y == null) return "";
                  return `${ctx.dataset.label}: ${fmt(ctx.parsed.y)}`;
                },
              },
            },
          },
          scales: {
            x: {
              grid: { color: "rgba(0,0,0,0.06)" },
              ticks: { font: { size: 11 } },
            },
            y: {
              grid: { color: "rgba(0,0,0,0.06)" },
              ticks: {
                font: { size: 11 },
                callback: (v) => fmtY(v as number),
              },
            },
          },
        },
      });
    }
  });

  // Destroy chart when leaving dashboard view or when component unmounts
  $effect(() => {
    if (activeView !== "dashboard" && chartInstance) {
      chartInstance.destroy();
      chartInstance = null;
    }
  });

  // ── Data loading ──────────────────────────────────────────────────────────────

  async function loadDashboard() {
    try {
      const [data, periods] = await Promise.all([
        invoke<DashboardData | null>("get_dashboard"),
        invoke<string[]>("get_available_periods"),
      ]);
      dashboard = data;
      pageState = data ? "dashboard" : "empty";
      availablePeriods = periods;
      if (periods.length > 0) {
        const prevTo = chartTo;
        chartTo = periods[periods.length - 1];
        chartFrom = periods.length >= 12 ? periods[periods.length - 12] : periods[0];
        console.log("[dashboard] periods:", periods, "chartFrom:", chartFrom, "chartTo:", chartTo, "prevTo:", prevTo);
        if (chartTo === prevTo && chartFrom) {
          invoke<ChartData | null>("get_chart_data", { from: chartFrom, to: chartTo })
            .then(data => { chartData = data; })
            .catch(() => {});
        }
      }
    } catch {
      dashboard = null;
      chartData = null;
      pageState = "empty";
    }
  }

  async function loadAccounts() {
    try {
      accounts = await invoke<Account[]>("get_accounts");
    } catch {
      // non-fatal
    }
  }

  async function loadTxPage(direction: "reset" | "append" | "prepend") {
    if (txLoading && direction !== "reset") return;
    txLoading = true;
    const myId = ++txRequestId;

    let offset = 0;
    if (direction === "append") offset = txOffset + txLoadedRows.length;
    else if (direction === "prepend") offset = Math.max(0, txOffset - TX_PAGE_SIZE);

    try {
      const page = await invoke<TransactionPage>("get_transactions", {
        filters: {
          account_id: txFilterAccount ? Number(txFilterAccount) : null,
          date_from: txFilterDateFrom || null,
          date_to: txFilterDateTo || null,
          category: txFilterCategory || null,
          kinds: txFilterKinds.length > 0 ? [...txFilterKinds] : null,
          offset,
          limit: TX_PAGE_SIZE,
        },
      });
      if (myId !== txRequestId) return;

      txTotal = page.total;

      if (direction === "reset") {
        txOffset = 0;
        txLoadedRows = page.rows;
      } else if (direction === "append") {
        const combined = [...txLoadedRows, ...page.rows];
        if (combined.length > TX_MAX_LOADED) {
          const drop = combined.length - TX_MAX_LOADED;
          txOffset += drop;
          txLoadedRows = combined.slice(drop);
        } else {
          txLoadedRows = combined;
        }
      } else {
        const prevScrollHeight = contentEl?.scrollHeight ?? 0;
        const prevScrollTop = contentEl?.scrollTop ?? 0;
        const combined = [...page.rows, ...txLoadedRows];
        txOffset = offset;
        txLoadedRows = combined.length > TX_MAX_LOADED ? combined.slice(0, TX_MAX_LOADED) : combined;
        await tick();
        if (contentEl && myId === txRequestId) {
          contentEl.scrollTop = prevScrollTop + (contentEl.scrollHeight - prevScrollHeight);
        }
      }
    } catch (err) {
      console.error("get_transactions failed:", err);
      if (myId === txRequestId && direction === "reset") txLoadedRows = [];
    } finally {
      if (myId === txRequestId) txLoading = false;
    }
  }

  function resetAndLoadTransactions() {
    txOffset = 0;
    txLoadedRows = [];
    txTotal = 0;
    loadTxPage("reset");
  }

  // Scroll-based pagination: append when near bottom, prepend when near top
  $effect(() => {
    if (activeView !== "transactions" || !contentEl) return;
    function onScroll() {
      if (txLoading) return;
      const el = contentEl!;
      const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 300;
      const nearTop = el.scrollTop < 300 && txOffset > 0;
      if (nearBottom) {
        if (txOffset + txLoadedRows.length < txTotal) loadTxPage("append");
      } else if (nearTop) {
        loadTxPage("prepend");
      }
    }
    contentEl.addEventListener("scroll", onScroll, { passive: true });
    return () => contentEl!.removeEventListener("scroll", onScroll);
  });

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
      chartInstance?.destroy();
    };
  });

  // ── Accounts view ─────────────────────────────────────────────────────────────

  function startEditName(acct: Account) {
    editingAccountId = acct.id;
    editingName = acct.display_name ?? acct.institution;
  }

  async function commitEditName(acct: Account) {
    if (editingAccountId !== acct.id) return;
    editingAccountId = null;
    const newName = editingName.trim() || null;
    if (newName === (acct.display_name ?? null) && newName === (acct.display_name)) return;
    try {
      await invoke("update_account", { id: acct.id, displayName: newName, color: acct.color });
      acct.display_name = newName;
      accounts = [...accounts];
    } catch {
      // revert
      editingName = acct.display_name ?? acct.institution;
    }
  }

  async function updateAccountColor(acct: Account, color: string) {
    try {
      await invoke("update_account", { id: acct.id, displayName: acct.display_name, color });
      acct.color = color;
      accounts = [...accounts];
      // refresh chart data so line colors update
      if (chartFrom && chartTo) {
        chartData = await invoke<ChartData | null>("get_chart_data", { from: chartFrom, to: chartTo });
      }
    } catch {
      // non-fatal
    }
  }

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
    "#396cd8", "#e05252", "#38a169", "#d69e2e", "#805ad5", "#319795",
  ];

  function seriesColorFallback(series: { account_type: string | null }, index: number): string {
    return series.account_type
      ? (TYPE_COLOR[series.account_type] ?? FALLBACK_COLORS[index % FALLBACK_COLORS.length])
      : FALLBACK_COLORS[index % FALLBACK_COLORS.length];
  }

  function accountLabel(s: AccountSeries): string {
    return `${s.display_name ?? s.institution} ···${s.account_number_last4}`;
  }

  function monthsBetween(from: string, to: string): string[] {
    const [fy, fm] = from.split("-").map(Number);
    const [ty, tm] = to.split("-").map(Number);
    const months: string[] = [];
    let y = fy, m = fm;
    while (y < ty || (y === ty && m <= tm)) {
      months.push(`${y}-${String(m).padStart(2, "0")}`);
      m++;
      if (m > 12) { m = 1; y++; }
    }
    return months;
  }

  const MONTH_ABBR = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];

  function xMonthLabel(period: string): string {
    const [y, m] = period.split("-");
    return `${MONTH_ABBR[parseInt(m) - 1]} '${y.slice(2)}`;
  }

  function xMonthName(period: string): string {
    return MONTH_ABBR[parseInt(period.split("-")[1]) - 1];
  }

  function fmtY(n: number): string {
    if (n === 0) return "$0";
    const sign = n < 0 ? "-" : "";
    return `${sign}$${(Math.abs(n) / 1000).toFixed(0)}k`;
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

  function handleNavClick(id: ActiveView) {
    activeView = id;
    if (id === "accounts") loadAccounts();
    if (id === "transactions") {
      if (accounts.length === 0) loadAccounts();
      resetAndLoadTransactions();
    }
  }
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
        onclick={() => handleNavClick(item.id)}
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

  <main class="content" bind:this={contentEl}>
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
      {#if pageState === "importing"}
        <div class="import-overlay">
          <div class="spinner" aria-label="Importing…"></div>
          <p class="hint">Extracting transactions…</p>
        </div>
      {/if}

      {#if dashboard}
        {#if activeView === "dashboard"}
          {#if chartData && chartData.account_series.length > 0 && chartFrom && chartTo}
            {@const cards = summaryCards(chartData)}
            <section class="chart-section" aria-label="Balance history">
              <div class="chart-header">
                <h2 class="section-title">Balance History</h2>
                <div class="chart-range">
                  <select class="period-select" bind:value={chartFrom}>
                    {#each availablePeriods.toReversed() as p}
                      <option value={p}>{xMonthName(p)} '{p.slice(2, 4)}</option>
                    {/each}
                  </select>
                  <span class="range-sep">—</span>
                  <select class="period-select" bind:value={chartTo}>
                    {#each availablePeriods.toReversed() as p}
                      <option value={p}>{xMonthName(p)} '{p.slice(2, 4)}</option>
                    {/each}
                  </select>
                </div>
              </div>
              <div class="chart-wrap">
                <canvas bind:this={canvasEl}></canvas>
              </div>
            </section>

            <!-- Summary cards — averages for selected range -->
            <section class="cards-section" aria-label="Monthly averages">
              <p class="cards-range-label">
                Averages for {xMonthName(chartFrom)} '{chartFrom.slice(2,4)} — {xMonthName(chartTo)} '{chartTo.slice(2,4)}
              </p>
              <div class="cards-row">
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
              </div>
            </section>
          {:else}
            <div class="chart-section">
              <p class="hint">No data in the last 12 months. Import a statement to get started.</p>
            </div>
          {/if}

        {:else if activeView === "accounts"}
          <section class="accounts-section" aria-label="Accounts">
            <h2 class="section-title">Accounts</h2>
            {#if accounts.length === 0}
              <p class="hint">No accounts yet. Import a statement to get started.</p>
            {:else}
              <ul class="accounts-list">
                {#each accounts as acct (acct.id)}
                  {@const swatchColor = acct.color ?? seriesColorFallback(acct, accounts.indexOf(acct))}
                  <li class="account-row">
                    <!-- Color swatch -->
                    <label class="swatch-wrap" title="Change color">
                      <span class="color-swatch" style="background:{swatchColor}"></span>
                      <input
                        type="color"
                        class="color-input"
                        value={swatchColor}
                        onchange={(e) => updateAccountColor(acct, (e.currentTarget as HTMLInputElement).value)}
                      />
                    </label>

                    <!-- Name (editable) -->
                    <div class="account-name-wrap">
                      {#if editingAccountId === acct.id}
                        <input
                          class="name-input"
                          type="text"
                          bind:value={editingName}
                          onblur={() => commitEditName(acct)}
                          onkeydown={(e) => { if (e.key === "Enter") commitEditName(acct); if (e.key === "Escape") editingAccountId = null; }}
                          use:focusOnMount
                        />
                      {:else}
                        <button class="name-display" onclick={() => startEditName(acct)} title="Click to edit name">
                          {acct.display_name ?? acct.institution}
                        </button>
                      {/if}
                      <span class="account-sub">···{acct.account_number_last4}{acct.account_type ? ` · ${acct.account_type.replace("_", " ")}` : ""}</span>
                    </div>

                    <!-- Balance -->
                    <div class="account-balance">
                      {#if acct.closing_balance != null}
                        <span class="balance-value">{fmt(acct.closing_balance)}</span>
                        <span class="balance-period">{acct.statement_period}</span>
                      {:else}
                        <span class="balance-value">—</span>
                      {/if}
                    </div>
                  </li>
                {/each}
              </ul>
            {/if}
          </section>
        {:else if activeView === "transactions"}
          <section class="transactions-section" aria-label="Transactions">
            <h2 class="section-title">Transactions</h2>
            <div class="tx-filters" role="search" aria-label="Filter transactions">
              <select
                class="filter-select"
                bind:value={txFilterAccount}
                onchange={resetAndLoadTransactions}
                aria-label="Filter by account"
              >
                <option value="">All accounts</option>
                {#each accounts as acct (acct.id)}
                  <option value={String(acct.id)}>
                    {acct.display_name ?? acct.institution} ···{acct.account_number_last4}
                  </option>
                {/each}
              </select>
              <input
                type="date"
                class="filter-input"
                value={txFilterDateFrom}
                oninput={(e) => { txFilterDateFrom = (e.currentTarget as HTMLInputElement).value; resetAndLoadTransactions(); }}
                aria-label="Date from"
                title="From date"
              />
              <input
                type="date"
                class="filter-input"
                value={txFilterDateTo}
                oninput={(e) => { txFilterDateTo = (e.currentTarget as HTMLInputElement).value; resetAndLoadTransactions(); }}
                aria-label="Date to"
                title="To date"
              />
              <input
                type="text"
                class="filter-input filter-category"
                placeholder="Category"
                bind:value={txFilterCategory}
                onchange={resetAndLoadTransactions}
                aria-label="Filter by category"
              />
              <fieldset class="filter-kinds" aria-label="Transaction type">
                <label class="kind-label">
                  <input type="checkbox" bind:group={txFilterKinds} value="debit" onchange={resetAndLoadTransactions} />
                  Debit
                </label>
                <label class="kind-label">
                  <input type="checkbox" bind:group={txFilterKinds} value="credit" onchange={resetAndLoadTransactions} />
                  Credit
                </label>
                <label class="kind-label">
                  <input type="checkbox" bind:group={txFilterKinds} value="transfer" onchange={resetAndLoadTransactions} />
                  Transfer
                </label>
              </fieldset>
            </div>
            <div class="tx-count-row">
              {#if txLoading && txLoadedRows.length === 0}
                <span class="hint">Loading…</span>
              {:else}
                <span class="hint">
                  Found {txTotal.toLocaleString()} transaction{txTotal === 1 ? "" : "s"}
                  {#if txOffset > 0 || txOffset + txLoadedRows.length < txTotal}
                    <span class="tx-window-hint">
                      (showing {(txOffset + 1).toLocaleString()}–{(txOffset + txLoadedRows.length).toLocaleString()})
                    </span>
                  {/if}
                </span>
                {#if txLoading}
                  <span class="tx-loading-inline">Loading…</span>
                {/if}
              {/if}
            </div>
            {#if txLoadedRows.length === 0 && !txLoading}
              <div class="tx-empty">
                <p class="hint">No transactions match the current filters.</p>
              </div>
            {:else}
              <table class="tx-table">
                <thead>
                  <tr>
                    <th>Date</th>
                    <th>Description</th>
                    <th>Category</th>
                    <th>Account</th>
                    <th class="num-col">Amount</th>
                  </tr>
                </thead>
                <tbody>
                  {#each txLoadedRows as tx (tx.id)}
                    <tr>
                      <td class="tx-date">{tx.date}</td>
                      <td class="tx-desc">{tx.description}</td>
                      <td class="tx-cat">{tx.category}</td>
                      <td class="tx-acct">{tx.institution} ···{tx.account_number_last4}</td>
                      <td class="num-col" class:tx-debit={tx.kind === "debit"} class:tx-credit={tx.kind === "credit"}>
                        {tx.kind === "debit" ? "−" : "+"}{fmt(tx.amount)}
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}
          </section>
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

  {#if importBanner && lastImport}
    <div class="import-toast">
      <svg class="icon-sm success-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
        <polyline points="22 4 12 14.01 9 11.01" />
      </svg>
      {importBannerText(lastImport)}
    </div>
  {/if}
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
    padding: 0.75rem 0 1rem;
    gap: 0.25rem;
    background: #ebebeb;
    border-right: 1px solid #ddd;
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
    transition: background 0.15s, color 0.15s;
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

  /* ── State card ── */

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

  /* ── Toast ── */

  .import-toast {
    position: fixed;
    bottom: 1.5rem;
    right: 1.5rem;
    max-width: 360px;
    background: rgba(56, 161, 105, 0.12);
    border: 1px solid #38a169;
    border-radius: 8px;
    padding: 0.6rem 1rem;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.9rem;
    color: #276749;
    z-index: 200;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.12);
    animation: toast-in 0.25s ease-out;
  }

  @keyframes toast-in {
    from { transform: translateX(120%); opacity: 0; }
    to   { transform: translateX(0);    opacity: 1; }
  }

  @media (prefers-color-scheme: dark) {
    .import-toast {
      background: rgba(56, 161, 105, 0.15);
      color: #68d391;
      box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
    }
  }

  /* ── Chart ── */

  .chart-section {
    width: 100%;
    max-width: 900px;
    margin-bottom: 1rem;
  }

  .chart-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.75rem;
  }

  .chart-header .section-title {
    margin-bottom: 0;
  }

  .chart-range {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .period-select {
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
    font-size: 0.78rem;
    font-weight: 500;
    color: #555;
    background: transparent;
    border: none;
    border-bottom: 1px solid #ccc;
    padding: 0.1rem 0.2rem;
    cursor: pointer;
    outline: none;
  }

  .period-select:focus {
    border-bottom-color: #396cd8;
  }

  @media (prefers-color-scheme: dark) {
    .period-select {
      color: #aaa;
      border-bottom-color: #444;
    }
    .period-select:focus {
      border-bottom-color: #396cd8;
    }
  }

  .range-sep {
    font-size: 0.78rem;
    color: #aaa;
  }

  .chart-wrap {
    width: 100%;
    height: 260px;
    position: relative;
  }

  .chart-wrap canvas {
    width: 100% !important;
    height: 100% !important;
  }

  /* ── Summary cards ── */

  .cards-section {
    width: 100%;
    max-width: 900px;
    margin-bottom: 2rem;
  }

  .cards-range-label {
    font-size: 0.75rem;
    color: #888;
    margin: 0 0 0.6rem;
    font-weight: 500;
  }

  @media (prefers-color-scheme: dark) {
    .cards-range-label { color: #777; }
  }

  .cards-row {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
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

  .card.net-positive { border-color: rgba(56, 161, 105, 0.4); }
  .card.net-negative { border-color: rgba(229, 62, 62, 0.4); }

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
    .card-label, .card-sub { color: #aaa; }
  }

  /* ── Accounts list ── */

  .accounts-section {
    width: 100%;
    max-width: 700px;
  }

  .accounts-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .account-row {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.75rem 0;
    border-bottom: 1px solid #e2e8f0;
  }

  @media (prefers-color-scheme: dark) {
    .account-row { border-bottom-color: #2d3748; }
  }

  .swatch-wrap {
    cursor: pointer;
    flex-shrink: 0;
    position: relative;
    display: flex;
    align-items: center;
  }

  .color-swatch {
    display: block;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 2px solid rgba(0,0,0,0.15);
    transition: transform 0.12s;
  }

  .swatch-wrap:hover .color-swatch {
    transform: scale(1.2);
  }

  .color-input {
    position: absolute;
    width: 0;
    height: 0;
    opacity: 0;
    pointer-events: none;
  }

  .account-name-wrap {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }

  .name-display {
    all: unset;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    border-bottom: 1px dashed transparent;
    transition: border-color 0.15s;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .name-display:hover {
    border-bottom-color: #aaa;
  }

  .name-input {
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
    font-size: 0.9rem;
    font-weight: 600;
    border: none;
    border-bottom: 1px solid #396cd8;
    outline: none;
    background: transparent;
    color: inherit;
    padding: 0;
    width: 100%;
  }

  .account-sub {
    font-size: 0.75rem;
    color: #888;
  }

  @media (prefers-color-scheme: dark) {
    .account-sub { color: #777; }
  }

  .account-balance {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    flex-shrink: 0;
  }

  .balance-value {
    font-size: 0.95rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .balance-period {
    font-size: 0.72rem;
    color: #888;
  }

  @media (prefers-color-scheme: dark) {
    .balance-period { color: #777; }
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

  .nav-bottom { margin-top: auto; }

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

  .field-input:focus { border-color: #396cd8; }

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

  button:not(.nav-btn):not(.name-display) {
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

  button:not(.nav-btn):not(.name-display):hover {
    background-color: #2d5bc7;
  }

  /* ── Transactions ── */

  .transactions-section {
    width: 100%;
    max-width: 960px;
  }

  .tx-filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.75rem;
  }

  .filter-select,
  .filter-input {
    font-size: 0.85rem;
    padding: 0.35em 0.6em;
    border: 1px solid #ccc;
    border-radius: 6px;
    background: #fff;
    color: inherit;
    min-width: 120px;
  }

  .filter-category {
    min-width: 160px;
  }

  @media (prefers-color-scheme: dark) {
    .filter-select,
    .filter-input {
      background: #222;
      border-color: #444;
      color: #f6f6f6;
    }
  }

  .filter-kinds {
    all: unset;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    border: 1px solid #ccc;
    border-radius: 6px;
    padding: 0.3em 0.7em;
    font-size: 0.85rem;
  }

  @media (prefers-color-scheme: dark) {
    .filter-kinds {
      border-color: #444;
    }
  }

  .kind-label {
    display: flex;
    align-items: center;
    gap: 0.3em;
    cursor: pointer;
    user-select: none;
  }

  .tx-count-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 0.5rem;
    min-height: 1.4rem;
  }

  .tx-window-hint {
    color: #aaa;
  }

  .tx-loading-inline {
    font-size: 0.8rem;
    color: #888;
  }

  .tx-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.88rem;
  }

  .tx-table th {
    text-align: left;
    padding: 0.5rem 0.75rem;
    border-bottom: 2px solid #e0e0e0;
    font-weight: 600;
    color: #666;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    white-space: nowrap;
  }

  .tx-table td {
    padding: 0.45rem 0.75rem;
    border-bottom: 1px solid #f0f0f0;
    vertical-align: middle;
  }

  .tx-table tr:last-child td {
    border-bottom: none;
  }

  .tx-date {
    white-space: nowrap;
    color: #888;
    font-size: 0.82rem;
    font-variant-numeric: tabular-nums;
  }

  .tx-desc {
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tx-cat {
    color: #888;
    font-size: 0.83rem;
  }

  .tx-acct {
    color: #888;
    font-size: 0.83rem;
    white-space: nowrap;
  }

  .num-col {
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .tx-debit {
    color: #e05252;
  }

  .tx-credit {
    color: #38a169;
  }

  .tx-empty {
    padding: 3rem 0;
    text-align: center;
  }

  @media (prefers-color-scheme: dark) {
    .tx-table th {
      border-bottom-color: #333;
      color: #888;
    }

    .tx-table td {
      border-bottom-color: #222;
    }
  }
</style>
