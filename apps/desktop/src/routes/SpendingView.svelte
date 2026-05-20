<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onDestroy, tick } from "svelte";
  import {
    Chart,
    DoughnutController,
    ArcElement,
    Tooltip,
    Legend,
  } from "chart.js";
  import type { Account, CategoryGroup } from "$lib/types";

  Chart.register(DoughnutController, ArcElement, Tooltip, Legend);

  interface CategorySpend {
    category: string;
    group_id: number | null;
    total: number;
    percentage: number;
  }

  const ALL_GROUPS = "__all_groups__";
  const ALL_CATEGORIES = "__all_categories__";

  type Period = "30d" | "90d" | "12m" | "custom";
  type SortCol = "category" | "total" | "percentage";
  type SortDir = "asc" | "desc";

  let {
    accounts,
    active = true,
    dataVersion = 0,
    onCategoryClick,
  }: {
    accounts: Account[];
    active?: boolean;
    dataVersion?: number;
    onCategoryClick?: (args: {
      category: string;
      from: string;
      to: string;
      accountId: string;
    }) => void;
  } = $props();

  let period = $state<Period>("30d");
  let customFrom = $state("");
  let customTo = $state("");
  let accountId = $state("");
  let groups = $state<CategoryGroup[]>([]);
  let groupSelection = $state<string>(ALL_GROUPS);
  let rows = $state<CategorySpend[]>([]);
  let loading = $state(false);
  let sortCol = $state<SortCol>("total");
  let sortDir = $state<SortDir>("desc");

  const groupBy = $derived(groupSelection === ALL_GROUPS ? "group" : "category");
  const groupIdFilter = $derived(
    groupSelection === ALL_GROUPS || groupSelection === ALL_CATEGORIES
      ? null
      : Number(groupSelection),
  );
  const selectedGroupName = $derived(
    groupIdFilter == null
      ? null
      : (groups.find((g) => g.id === groupIdFilter)?.name ?? null),
  );

  let canvasEl = $state<HTMLCanvasElement | null>(null);
  let chart: Chart<"doughnut"> | null = null;
  let loadedVersion = 0;

  const COLORS = [
    "#396cd8", "#e05252", "#38a169", "#d69e2e", "#805ad5",
    "#319795", "#dd6b20", "#e53e3e", "#3182ce", "#6b46c1",
  ];

  function dateRange(): { from: string; to: string } {
    const now = new Date();
    const pad = (n: number) => String(n).padStart(2, "0");
    const fmt = (d: Date) =>
      `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

    if (period === "30d") {
      const from = new Date(now);
      from.setDate(from.getDate() - 30);
      return { from: fmt(from), to: fmt(now) };
    }
    if (period === "90d") {
      const from = new Date(now);
      from.setDate(from.getDate() - 90);
      return { from: fmt(from), to: fmt(now) };
    }
    if (period === "12m") {
      const from = new Date(now);
      from.setFullYear(from.getFullYear() - 1);
      return { from: fmt(from), to: fmt(now) };
    }
    return { from: customFrom, to: customTo };
  }

  async function load() {
    const { from, to } = dateRange();
    if (!from || !to) return;

    loading = true;
    try {
      rows = await invoke<CategorySpend[]>("get_spending_by_category", {
        filters: {
          date_from: from,
          date_to: to,
          account_id: accountId ? Number(accountId) : null,
          group_by: groupBy,
          group_id: groupIdFilter,
        },
      });
      loadedVersion = dataVersion;
    } catch (e) {
      console.error("spending fetch failed", e);
      rows = [];
    } finally {
      loading = false;
    }
  }

  async function loadGroups() {
    try {
      groups = await invoke<CategoryGroup[]>("list_category_groups");
    } catch (e) {
      console.error("group fetch failed", e);
      groups = [];
    }
  }

  function handleRowClick(row: CategorySpend) {
    // In group mode, a row with group_id drills into that group; an ungrouped row
    // is just a raw category — navigate to Transactions like in category mode.
    if (groupBy === "group" && row.group_id != null) {
      groupSelection = String(row.group_id);
      return;
    }
    if (onCategoryClick) {
      const { from, to } = dateRange();
      onCategoryClick({ category: row.category, from, to, accountId });
    }
  }

  function updateChart() {
    if (!canvasEl) {
      chart?.destroy();
      chart = null;
      return;
    }
    const labels = rows.map((r) => r.category || "(uncategorized)");
    const data = rows.map((r) => r.total);
    const colors = rows.map((_, i) => COLORS[i % COLORS.length]);

    // Destroy and recreate if the canvas element changed (happens when the DOM
    // remounts after a loading/empty state transition).
    if (chart && chart.canvas !== canvasEl) {
      chart.destroy();
      chart = null;
    }

    if (chart) {
      chart.data.labels = labels;
      chart.data.datasets[0].data = data;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (chart.data.datasets[0] as any).backgroundColor = colors;
      chart.update();
    } else {
      chart = new Chart(canvasEl, {
        type: "doughnut",
        data: {
          labels,
          datasets: [{ data, backgroundColor: colors, borderWidth: 1 }],
        },
        options: {
          responsive: true,
          maintainAspectRatio: false,
          onClick: (_evt, els) => {
            const el = els[0];
            if (el && rows[el.index]) handleRowClick(rows[el.index]);
          },
          plugins: {
            legend: {
              position: "right",
              labels: { boxWidth: 12, padding: 14, font: { size: 12 } },
              onClick: (_evt, item) => {
                const idx = item.index ?? -1;
                if (idx >= 0 && rows[idx]) handleRowClick(rows[idx]);
              },
            },
            tooltip: {
              callbacks: {
                label: (ctx) => {
                  const val = ctx.parsed as number;
                  const pct = rows[ctx.dataIndex]?.percentage ?? 0;
                  return ` ${val.toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 })} (${pct}%)`;
                },
              },
            },
          },
        },
      });
    }
  }

  $effect(() => {
    // Re-run whenever filters change
    const _ = period,
      __ = accountId,
      ___ = customFrom,
      ____ = customTo,
      _____ = groupSelection;
    load();
  });

  $effect(() => {
    // Refresh group list when transactions change so newly-added categories show up.
    const _ = dataVersion;
    loadGroups();
  });

  $effect(() => {
    // Update chart whenever rows change (after load)
    updateChart();
  });

  $effect(() => {
    // When the view becomes visible again after being CSS-hidden, Chart.js needs
    // to recalculate dimensions (it had zero size while display:none). Also
    // refresh if transactions have been mutated since the last load.
    if (active) {
      if (dataVersion !== loadedVersion) {
        load();
      }
      tick().then(() => chart?.resize());
    }
  });

  onDestroy(() => {
    chart?.destroy();
    chart = null;
  });

  function fmt(n: number) {
    return n.toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 });
  }

  function sorted(data: CategorySpend[]): CategorySpend[] {
    return [...data].sort((a, b) => {
      const av = a[sortCol];
      const bv = b[sortCol];
      if (typeof av === "string" && typeof bv === "string") {
        return sortDir === "asc" ? av.localeCompare(bv) : bv.localeCompare(av);
      }
      return sortDir === "asc" ? (av as number) - (bv as number) : (bv as number) - (av as number);
    });
  }

  function toggleSort(col: SortCol) {
    if (sortCol === col) {
      sortDir = sortDir === "asc" ? "desc" : "asc";
    } else {
      sortCol = col;
      sortDir = col === "category" ? "asc" : "desc";
    }
  }

  function sortIndicator(col: SortCol): string {
    if (sortCol !== col) return "";
    return sortDir === "asc" ? " ▲" : " ▼";
  }
</script>

<section class="spending-section" aria-label="Spending by category">
  <div class="spending-header">
    <h2 class="section-title">Spending by Category</h2>
    <div class="spending-controls">
      <div class="period-tabs">
        {#each (["30d", "90d", "12m", "custom"] as Period[]) as p}
          <button
            class="period-tab"
            class:active={period === p}
            onclick={() => { period = p; }}
          >{p === "30d" ? "30 days" : p === "90d" ? "90 days" : p === "12m" ? "12 months" : "Custom"}</button>
        {/each}
      </div>

      {#if period === "custom"}
        <div class="custom-range">
          <input type="date" bind:value={customFrom} class="date-input" />
          <span class="range-sep">—</span>
          <input type="date" bind:value={customTo} class="date-input" />
        </div>
      {/if}

      {#if accounts.length > 0}
        <select class="account-select" bind:value={accountId}>
          <option value="">All accounts</option>
          {#each accounts as acct (acct.id)}
            <option value={String(acct.id)}>
              {acct.display_name ?? acct.institution} ···{acct.account_number_last4}
            </option>
          {/each}
        </select>
      {/if}

      <select class="account-select" bind:value={groupSelection} aria-label="Group view">
        <option value={ALL_GROUPS}>All groups</option>
        <option value={ALL_CATEGORIES}>All categories</option>
        {#if groups.length > 0}
          <option disabled>──────</option>
          {#each groups as g (g.id)}
            <option value={String(g.id)}>{g.name}</option>
          {/each}
        {/if}
      </select>
    </div>
  </div>

  {#if selectedGroupName}
    <div class="breadcrumb">
      <button type="button" class="crumb-link" onclick={() => { groupSelection = ALL_GROUPS; }}>← All groups</button>
      <span class="crumb-sep">/</span>
      <span class="crumb-current">{selectedGroupName}</span>
    </div>
  {/if}

  {#if loading}
    <div class="spending-loading">
      <div class="spinner" aria-label="Loading…"></div>
    </div>
  {:else if rows.length === 0}
    <p class="hint">No spending data for this period.</p>
  {:else}
    <div class="spending-body">
      <div class="chart-wrap">
        <canvas bind:this={canvasEl}></canvas>
      </div>

      <table class="spend-table">
        <thead>
          <tr>
            <th class="col-cat" onclick={() => toggleSort("category")}>{groupBy === "group" ? "Group" : "Category"}{sortIndicator("category")}</th>
            <th class="col-num" onclick={() => toggleSort("total")}>Total{sortIndicator("total")}</th>
            <th class="col-pct" onclick={() => toggleSort("percentage")}>%{sortIndicator("percentage")}</th>
          </tr>
        </thead>
        <tbody>
          {#each sorted(rows) as row, i (row.category)}
            {@const color = COLORS[rows.indexOf(row) % COLORS.length]}
            <tr>
              <td class="col-cat">
                <span class="cat-swatch" style="background:{color}"></span>
                <button
                  type="button"
                  class="cat-link"
                  onclick={() => handleRowClick(row)}
                >{row.category || "(uncategorized)"}</button>
              </td>
              <td class="col-num">{fmt(row.total)}</td>
              <td class="col-pct">{row.percentage}%</td>
            </tr>
          {/each}
        </tbody>
        <tfoot>
          <tr>
            <td class="col-cat foot-label">Total</td>
            <td class="col-num">{fmt(rows.reduce((s, r) => s + r.total, 0))}</td>
            <td class="col-pct"></td>
          </tr>
        </tfoot>
      </table>
    </div>
  {/if}
</section>

<style>
  .spending-section {
    width: 100%;
    max-width: 900px;
  }

  .spending-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 0.75rem;
    margin-bottom: 1.25rem;
  }

  .spending-header .section-title {
    margin-bottom: 0;
  }

  .spending-controls {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.75rem;
  }

  .period-tabs {
    display: flex;
    gap: 0.2rem;
  }

  .period-tab {
    padding: 0.25rem 0.65rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    background: transparent;
    font-size: 0.78rem;
    font-weight: 500;
    color: #555;
    cursor: pointer;
    transition: background 0.1s, color 0.1s;
  }

  .period-tab:hover { background: #f0f0f0; }
  .period-tab.active { background: #396cd8; border-color: #396cd8; color: #fff; }

  .custom-range {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .date-input {
    font-size: 0.78rem;
    border: none;
    border-bottom: 1px solid #ccc;
    background: transparent;
    padding: 0.1rem 0.2rem;
    outline: none;
    color: #555;
    font-family: inherit;
    cursor: pointer;
  }

  .date-input:focus { border-bottom-color: #396cd8; }

  .account-select {
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

  .account-select:focus { border-bottom-color: #396cd8; }

  .range-sep { font-size: 0.78rem; color: #aaa; }

  .breadcrumb {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.82rem;
    color: #555;
    margin-bottom: 0.75rem;
  }

  .crumb-link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: #396cd8;
    cursor: pointer;
  }

  .crumb-link:hover { text-decoration: underline; }

  .crumb-sep { color: #aaa; }
  .crumb-current { font-weight: 500; }

  @media (prefers-color-scheme: dark) {
    .breadcrumb { color: #aaa; }
    .crumb-sep { color: #555; }
  }

  .spending-loading {
    display: flex;
    justify-content: center;
    padding: 3rem 0;
  }

  .spending-body {
    display: flex;
    gap: 2rem;
    align-items: flex-start;
  }

  .chart-wrap {
    flex: 0 0 260px;
    height: 260px;
    position: relative;
  }

  .chart-wrap canvas {
    width: 100% !important;
    height: 100% !important;
  }

  /* ── Table ── */

  .spend-table {
    flex: 1;
    border-collapse: collapse;
    font-size: 0.875rem;
    min-width: 260px;
  }

  .spend-table thead th {
    text-align: left;
    font-size: 0.75rem;
    font-weight: 600;
    color: #888;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0 0.5rem 0.5rem;
    border-bottom: 1px solid #e2e8f0;
    cursor: pointer;
    user-select: none;
    white-space: nowrap;
  }

  .spend-table thead th:hover { color: #555; }

  .spend-table tbody tr:hover td { background: #f7f9fc; }

  .spend-table td {
    padding: 0.45rem 0.5rem;
    border-bottom: 1px solid #f0f0f0;
  }

  .spend-table tfoot td {
    padding: 0.5rem 0.5rem 0;
    border-top: 1px solid #e2e8f0;
    font-weight: 600;
  }

  .col-cat { width: 100%; }
  .col-num { text-align: right; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .col-pct { text-align: right; white-space: nowrap; color: #888; font-variant-numeric: tabular-nums; padding-left: 1rem; }

  .foot-label { color: #555; }

  .cat-link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }

  .cat-link:hover {
    color: #396cd8;
    text-decoration: underline;
  }

  .cat-swatch {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    margin-right: 0.4rem;
    vertical-align: middle;
    flex-shrink: 0;
  }

  /* ── Dark mode ── */

  @media (prefers-color-scheme: dark) {
    .period-tab { border-color: #444; color: #aaa; }
    .period-tab:hover { background: #2a2a2a; }
    .period-tab.active { background: #396cd8; border-color: #396cd8; color: #fff; }
    .date-input { color: #aaa; border-bottom-color: #444; }
    .date-input:focus { border-bottom-color: #396cd8; }
    .account-select { color: #aaa; border-bottom-color: #444; }
    .account-select:focus { border-bottom-color: #396cd8; }
    .spend-table thead th { color: #666; border-bottom-color: #333; }
    .spend-table thead th:hover { color: #aaa; }
    .spend-table tbody tr:hover td { background: #1e1e1e; }
    .spend-table td { border-bottom-color: #2a2a2a; }
    .spend-table tfoot td { border-top-color: #333; }
    .col-pct { color: #666; }
    .foot-label { color: #aaa; }
  }
</style>
