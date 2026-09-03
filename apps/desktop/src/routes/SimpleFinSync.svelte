<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { Account } from "$lib/types";

  interface AccountSyncResult {
    account_id: number;
    institution: string;
    account_number_last4: string;
    inserted: number;
    already_present: number;
    skipped_before_cutover: number;
    skipped_pending: number;
    periods: string[];
  }

  interface UnmappedAccount {
    simplefin_id: string;
    institution: string;
    name: string;
    balance: number | null;
  }

  interface SyncReport {
    accounts: AccountSyncResult[];
    unmapped: UnmappedAccount[];
    errors: string[];
    categorized_by_rule: number;
    categorized_by_model: number;
    uncategorized: number;
  }

  interface NewAccountForm {
    institution: string;
    last4: string;
    accountType: string;
  }

  const NEW = "__new__";

  let {
    accounts,
    configured,
    onSynced,
    onAccountsChanged,
  }: {
    accounts: Account[];
    configured: boolean;
    onSynced: () => void;
    onAccountsChanged: () => void;
  } = $props();

  let syncing = $state(false);
  let report = $state<SyncReport | null>(null);
  let error = $state<string | null>(null);
  let choice = $state<Record<string, string>>({});
  let forms = $state<Record<string, NewAccountForm>>({});
  let mappingId = $state<string | null>(null);

  const mapped = $derived(accounts.filter((a) => a.simplefin_id));
  const unmappedAccounts = $derived(accounts.filter((a) => !a.simplefin_id));

  function label(a: Account): string {
    return `${a.display_name ?? a.institution} ···${a.account_number_last4}`;
  }

  function guessLast4(name: string): string {
    const m = name.match(/(\d{4})\D*$/);
    return m ? m[1] : "";
  }

  function ensureForm(u: UnmappedAccount) {
    if (!forms[u.simplefin_id]) {
      forms[u.simplefin_id] = { institution: u.institution, last4: guessLast4(u.name), accountType: "" };
    }
    if (!choice[u.simplefin_id]) {
      choice[u.simplefin_id] = unmappedAccounts.length > 0 ? String(unmappedAccounts[0].id) : NEW;
    }
  }

  async function sync() {
    syncing = true;
    error = null;
    try {
      report = await invoke<SyncReport>("sync_simplefin");
      for (const u of report.unmapped) ensureForm(u);
      onSynced();
    } catch (e) {
      error = String(e);
    } finally {
      syncing = false;
    }
  }

  async function mapAccount(u: UnmappedAccount) {
    mappingId = u.simplefin_id;
    error = null;
    try {
      const selected = choice[u.simplefin_id];
      const form = forms[u.simplefin_id];
      await invoke("map_simplefin_account", {
        simplefinId: u.simplefin_id,
        accountId: selected === NEW ? null : Number(selected),
        institution: selected === NEW ? form.institution : null,
        accountNumberLast4: selected === NEW ? form.last4 : null,
        accountType: selected === NEW && form.accountType ? form.accountType : null,
      });
      onAccountsChanged();
      await sync();
    } catch (e) {
      error = String(e);
    } finally {
      mappingId = null;
    }
  }

  async function unmap(a: Account) {
    error = null;
    try {
      await invoke("unmap_simplefin_account", { accountId: a.id });
      onAccountsChanged();
    } catch (e) {
      error = String(e);
    }
  }
</script>

<section class="sync" aria-label="SimpleFIN sync">
  <div class="sync-head">
    <h2 class="sync-title">SimpleFIN Sync</h2>
    <button type="button" onclick={sync} disabled={syncing || !configured}>
      {syncing ? "Syncing…" : "Sync now"}
    </button>
  </div>

  {#if !configured}
    <p class="muted">Add a SimpleFIN setup token or access URL in Settings to enable syncing.</p>
  {/if}

  {#if mapped.length > 0}
    <ul class="mapped">
      {#each mapped as a (a.id)}
        <li>
          <span>{label(a)}</span>
          <button type="button" class="link" onclick={() => unmap(a)}>stop syncing</button>
        </li>
      {/each}
    </ul>
  {/if}

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if report}
    {#if report.accounts.length > 0}
      <table class="report">
        <thead>
          <tr><th>Account</th><th class="num">New</th><th class="num">Existing</th><th class="num">Skipped</th><th>Periods</th></tr>
        </thead>
        <tbody>
          {#each report.accounts as r (r.account_id)}
            <tr>
              <td>{r.institution} ···{r.account_number_last4}</td>
              <td class="num">{r.inserted}</td>
              <td class="num">{r.already_present}</td>
              <td class="num">{r.skipped_before_cutover + r.skipped_pending}</td>
              <td>{r.periods.join(", ")}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="muted">
        Categorized {report.categorized_by_rule} by rule, {report.categorized_by_model} by model, {report.uncategorized} left uncategorized.
      </p>
    {/if}

    {#each report.errors as e}
      <p class="error">{e}</p>
    {/each}

    {#each report.unmapped as u (u.simplefin_id)}
      <div class="unmapped">
        <p><strong>{u.institution}</strong> — {u.name}{#if u.balance !== null} (balance {u.balance.toFixed(2)}){/if}</p>
        <div class="row">
          <select bind:value={choice[u.simplefin_id]} aria-label="Map to account">
            {#each unmappedAccounts as a (a.id)}
              <option value={String(a.id)}>{label(a)}</option>
            {/each}
            <option value={NEW}>Create new account…</option>
          </select>
          {#if choice[u.simplefin_id] === NEW && forms[u.simplefin_id]}
            <input type="text" placeholder="Institution" bind:value={forms[u.simplefin_id].institution} aria-label="Institution" />
            <input type="text" placeholder="Last 4" maxlength="4" bind:value={forms[u.simplefin_id].last4} aria-label="Last 4 digits" />
            <select bind:value={forms[u.simplefin_id].accountType} aria-label="Account type">
              <option value="">Type…</option>
              <option value="checking">Checking</option>
              <option value="savings">Savings</option>
              <option value="credit_card">Credit card</option>
            </select>
          {/if}
          <button type="button" onclick={() => mapAccount(u)} disabled={mappingId === u.simplefin_id}>
            {mappingId === u.simplefin_id ? "Mapping…" : "Map"}
          </button>
        </div>
      </div>
    {/each}
  {/if}
</section>

<style>
  .sync { margin-bottom: 2rem; }
  .sync-head { display: flex; align-items: center; justify-content: space-between; gap: 1rem; margin-bottom: 0.5rem; }
  .sync-title { font-size: 1.1rem; font-weight: 600; margin: 0; }
  .muted { opacity: 0.7; font-size: 0.9rem; }
  .error { color: #c0392b; font-size: 0.9rem; }
  .mapped { list-style: none; padding: 0; margin: 0 0 0.75rem; font-size: 0.9rem; }
  .mapped li { display: flex; gap: 0.75rem; align-items: baseline; }
  .link { background: none; border: none; padding: 0; color: inherit; opacity: 0.7; text-decoration: underline; cursor: pointer; font-size: 0.85rem; }
  .report { width: 100%; border-collapse: collapse; font-size: 0.9rem; margin: 0.5rem 0; }
  .report th, .report td { text-align: left; padding: 0.3rem 0.5rem; border-bottom: 1px solid rgba(128, 128, 128, 0.25); }
  .num { text-align: right; }
  .unmapped { border: 1px solid rgba(128, 128, 128, 0.35); border-radius: 6px; padding: 0.75rem; margin: 0.75rem 0; }
  .unmapped p { margin: 0 0 0.5rem; }
  .row { display: flex; flex-wrap: wrap; gap: 0.5rem; align-items: center; }
  .row input[type="text"] { max-width: 10rem; }
</style>
