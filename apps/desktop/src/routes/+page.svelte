<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";

  type Status = "idle" | "loading" | "success" | "error";

  interface ImportSummary {
    institution: string;
    account_number_last4: string;
    statement_period: string;
    transaction_count: number;
  }

  interface Transaction {
    date: string;
    description: string;
    category: string;
    amount: number;
    type: "debit" | "credit";
  }

  let status = $state<Status>("idle");
  let summary = $state<ImportSummary | null>(null);
  let transactions = $state<Transaction[]>([]);
  let errorMessage = $state("");
  let dragging = $state(false);

  onMount(() => {
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
          status = "error";
          errorMessage =
            "Only PDF files are supported. Please drop a bank or credit card statement PDF.";
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
    // Real file paths come from the Tauri DragDrop event above.
    // This handler catches non-PDF drops in the browser environment.
    const file = e.dataTransfer?.files[0];
    if (file && !file.name.toLowerCase().endsWith(".pdf")) {
      status = "error";
      errorMessage =
        "Only PDF files are supported. Please drop a bank or credit card statement PDF.";
    }
  }

  async function runImport(path: string) {
    status = "loading";
    errorMessage = "";
    try {
      summary = await invoke<ImportSummary>("import_statement", { path });
      transactions = [];
      status = "success";
    } catch (e) {
      errorMessage = String(e);
      status = "error";
    }
  }

  function reset() {
    status = "idle";
    summary = null;
    transactions = [];
    errorMessage = "";
    dragging = false;
  }
</script>

<main class="container">
  {#if status === "idle" || status === "loading"}
    <div
      class="drop-zone"
      class:dragging
      class:loading={status === "loading"}
      ondragover={onDragOver}
      ondragleave={onDragLeave}
      ondrop={onDrop}
      role="region"
      aria-label="Drop zone for PDF statements"
    >
      {#if status === "loading"}
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
  {:else if status === "error"}
    <div class="state-card error-card">
      <svg class="icon error-icon" viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="12" cy="12" r="10" />
        <line x1="12" y1="8" x2="12" y2="12" />
        <line x1="12" y1="16" x2="12.01" y2="16" />
      </svg>
      <p class="headline">Import failed</p>
      <p class="hint">{errorMessage}</p>
      <button onclick={reset}>Try again</button>
    </div>
  {:else if status === "success" && summary}
    <div class="success-header">
      <svg class="icon success-icon" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
        <polyline points="22 4 12 14.01 9 11.01" />
      </svg>
      <div class="summary-text">
        <p class="headline">
          {summary.institution} ···{summary.account_number_last4}
        </p>
        <p class="hint">
          {summary.statement_period} · {summary.transaction_count} transactions imported
        </p>
      </div>
      <button class="secondary" onclick={reset}>Import another</button>
    </div>

    {#if transactions.length > 0}
      <table class="tx-table">
        <thead>
          <tr>
            <th>Date</th>
            <th>Description</th>
            <th>Category</th>
            <th class="amount-col">Amount</th>
            <th>Type</th>
          </tr>
        </thead>
        <tbody>
          {#each transactions as tx}
            <tr>
              <td>{tx.date}</td>
              <td>{tx.description}</td>
              <td>{tx.category}</td>
              <td class="amount-col">${tx.amount.toFixed(2)}</td>
              <td class:debit={tx.type === "debit"} class:credit={tx.type === "credit"}
                >{tx.type}</td
              >
            </tr>
          {/each}
        </tbody>
      </table>
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
    to {
      transform: rotate(360deg);
    }
  }

  /* ── State cards ── */
  .state-card {
    width: 100%;
    max-width: 560px;
    margin-top: 10vh;
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

  /* ── Success header ── */
  .success-header {
    width: 100%;
    max-width: 800px;
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 1rem 0 1.5rem;
  }

  .summary-text {
    flex: 1;
  }

  /* ── Typography ── */
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
    .hint {
      color: #aaa;
    }
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

  .error-icon {
    color: #e53e3e;
    opacity: 1;
  }

  .success-icon {
    color: #38a169;
    opacity: 1;
    width: 36px;
    height: 36px;
    flex-shrink: 0;
  }

  /* ── Buttons ── */
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

  button:hover {
    background-color: #2d5bc7;
  }

  button.secondary {
    background-color: transparent;
    border-color: #396cd8;
    color: #396cd8;
  }

  button.secondary:hover {
    background-color: rgba(57, 108, 216, 0.08);
  }

  /* ── Transaction table ── */
  .tx-table {
    width: 100%;
    max-width: 800px;
    border-collapse: collapse;
    font-size: 0.9rem;
  }

  .tx-table th,
  .tx-table td {
    padding: 0.6rem 0.75rem;
    text-align: left;
    border-bottom: 1px solid #e2e8f0;
  }

  @media (prefers-color-scheme: dark) {
    .tx-table th,
    .tx-table td {
      border-bottom-color: #2d3748;
    }
  }

  .tx-table th {
    font-weight: 600;
    color: #555;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .amount-col {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .debit {
    color: #e53e3e;
  }

  .credit {
    color: #38a169;
  }
</style>
