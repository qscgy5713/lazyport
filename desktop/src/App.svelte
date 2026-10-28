<script>
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  let ports = $state([]);
  let query = $state("");
  let auto = $state(true);
  let force = $state(false);
  let confirming = $state(null); // 等待二次確認的 port
  let checking = $state(null); // 正在查詢關閉建議的那一列
  let warn = $state(null); // 「不建議關閉」的加強警告:{ key, port, info }
  let message = $state("");
  let error = $state("");
  let expanded = $state(null); // 展開詳情的那一列(port-pid;同一個 pid 可能開多個 port,不能只用 pid)
  let details = $state({}); // pid -> { loading, data, error }
  let shortcut = $state("");
  let recording = $state(false);
  let shortcutError = $state("");
  let rules = $state([]); // 使用者的自訂說明
  let rulesError = $state("");
  let form = $state(null); // 新增 / 編輯自訂說明的表單

  // 列的唯一識別:同一個 pid 可能開多個 port,不能只用 pid
  const rowKey = (p) => `${p.port}-${p.pid}`;

  const shown = $derived(
    ports.filter((p) => {
      const q = query.trim().toLowerCase();
      return !q || String(p.port).includes(q) || p.command.toLowerCase().includes(q);
    }),
  );

  async function refresh() {
    try {
      ports = await invoke("list_ports");
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  // 每次都重新查詢(不用快取):pid 可能已被系統重新分配給別的程式,舊的建議不可信。
  // 查不到時回傳 null,改走一般確認流程
  async function ensureInfo(pid) {
    try {
      const data = await invoke("process_info", { pid });
      details[pid] = { data };
      return data;
    } catch {
      return null;
    }
  }

  async function kill(p) {
    if (checking) return;
    if (warn?.key === rowKey(p)) {
      warn = null; // 警告展開中再按一次 = 取消
      return;
    }
    warn = null;
    if (confirming !== p.port) {
      // 先查關閉建議:不建議關閉的程式改走加強警告,不會進入一般的「確定結束?」
      checking = rowKey(p);
      const info = await ensureInfo(p.pid);
      checking = null;
      if (info?.advice === "avoid") {
        warn = { key: rowKey(p), port: p.port, info };
      } else {
        confirming = p.port;
      }
      return;
    }
    confirming = null;
    await doKill(p.port);
  }

  async function doKill(port) {
    warn = null;
    try {
      const killed = await invoke("kill_port", { port, force });
      message = `已結束 ${killed.map((k) => `${k.command} (pid ${k.pid})`).join("、")}`;
      error = "";
    } catch (e) {
      error = String(e);
      message = "";
    }
    await refresh();
  }

  const KIND_LABEL = { cwd: "工作目錄", args: "指令列包含", command: "程式名稱" };
  const ADVICE_LABEL = { safe: "可以關閉", caution: "關閉前請留意", avoid: "不建議關閉" };

  async function loadRules() {
    try {
      rules = await invoke("list_custom_hints");
      rulesError = "";
    } catch (e) {
      rulesError = String(e);
    }
  }

  // 表單預設值:專案資料夾最能辨識自己的程式;工作目錄太籠統(如家目錄)時改用程式名稱
  function openForm(p, d, rule) {
    const args = d.args.split(/\s+/).slice(1).join(" ") || d.args;
    const defaults = { cwd: d.cwd, command: p.command, args: args.slice(0, 80) };
    if (rule) {
      form = { ...rule, defaults, error: "" };
    } else {
      const kind = d.cwd_generic ? "command" : "cwd";
      form = { id: null, kind, pattern: defaults[kind], desc: "", advice: "caution", note: "", defaults, error: "" };
    }
  }

  async function reloadAfterRuleChange() {
    await loadRules();
    details = {};
    if (expanded) {
      const pid = Number(expanded.split("-")[1]);
      try {
        details[pid] = { data: await invoke("process_info", { pid }) };
      } catch (e) {
        details[pid] = { error: String(e) };
      }
    }
  }

  async function saveForm() {
    const { id, kind, pattern, desc, advice, note } = form;
    try {
      await invoke("save_custom_hint", { id, kind, pattern, desc, advice, note });
      form = null;
      await reloadAfterRuleChange();
    } catch (e) {
      form.error = String(e);
    }
  }

  async function removeRule(id) {
    try {
      await invoke("delete_custom_hint", { id });
      await reloadAfterRuleChange();
    } catch (e) {
      rulesError = String(e);
    }
  }

  async function toggleDetail(p) {
    form = null;
    const pid = p.pid;
    if (expanded === rowKey(p)) {
      expanded = null;
      return;
    }
    expanded = rowKey(p);
    details[pid] = { loading: true };
    try {
      details[pid] = { data: await invoke("process_info", { pid }) };
    } catch (e) {
      details[pid] = { error: String(e) };
    }
  }

  const SYMBOLS = { CommandOrControl: "⌘", Command: "⌘", Control: "⌃", Alt: "⌥", Shift: "⇧" };
  const pretty = (s) =>
    s
      .split("+")
      .map((k) => SYMBOLS[k] ?? k)
      .join("");

  // KeyboardEvent.code -> 快捷鍵字串中的按鍵名稱
  function keyName(code) {
    let m;
    if ((m = code.match(/^Key([A-Z])$/))) return m[1];
    if ((m = code.match(/^Digit(\d)$/))) return m[1];
    if (/^F\d{1,2}$/.test(code)) return code;
    if (code === "Space") return "Space";
    return null;
  }

  async function onKeydown(e) {
    if (!recording) return;
    e.preventDefault();
    if (e.key === "Escape") {
      recording = false;
      shortcutError = "";
      return;
    }
    if (["Meta", "Control", "Alt", "Shift"].includes(e.key)) return;
    const key = keyName(e.code);
    const mods = [e.metaKey && "Command", e.ctrlKey && "Control", e.altKey && "Alt", e.shiftKey && "Shift"].filter(Boolean);
    if (!key) {
      shortcutError = "只支援字母、數字、F 鍵與空白鍵";
      return;
    }
    if (!mods.some((m) => m !== "Shift")) {
      shortcutError = "請至少搭配 ⌘、⌃ 或 ⌥ 其中一個";
      return;
    }
    try {
      shortcut = await invoke("set_shortcut", { shortcut: [...mods, key].join("+") });
      shortcutError = "";
      recording = false;
    } catch (err) {
      shortcutError = String(err);
    }
  }

  onMount(() => {
    invoke("get_shortcut").then((s) => (shortcut = s));
    loadRules();
    refresh();
    const timer = setInterval(() => auto && refresh(), 3000);
    return () => clearInterval(timer);
  });
</script>

<svelte:window onkeydown={onKeydown} />

<main>
  <header>
    <h1>lazyport</h1>
    <input type="search" placeholder="搜尋 port 或程式名稱" bind:value={query} />
    <label><input type="checkbox" bind:checked={auto} /> 自動更新</label>
    <label><input type="checkbox" bind:checked={force} /> 強制結束</label>
    <button onclick={refresh}>重新整理</button>
  </header>

  {#if error}<p class="error">{error}</p>{/if}
  {#if message}<p class="ok">{message}</p>{/if}

  <table>
    <thead>
      <tr><th>Port</th><th>PID</th><th>程式</th><th>位址</th><th></th></tr>
    </thead>
    <tbody>
      {#each shown as p (rowKey(p))}
        <tr>
          <td class="port">{p.port}</td>
          <td>{p.pid}</td>
          <td>
            <button class="link" onclick={() => toggleDetail(p)} title="這是什麼程式?">
              {expanded === rowKey(p) ? "▾" : "▸"} {p.command}
            </button>
          </td>
          <td class="muted">{p.addr}</td>
          <td class="action">
            <button
              class:danger={confirming === p.port}
              disabled={checking === rowKey(p)}
              onclick={() => kill(p)}
              onblur={() => (confirming = null)}
            >
              {#if checking === rowKey(p)}檢查中…{:else if warn?.key === rowKey(p)}取消{:else if confirming === p.port}確定結束?{:else}結束{/if}
            </button>
          </td>
        </tr>
        {#if warn?.key === rowKey(p)}
          <tr class="warn">
            <td colspan="5">
              <strong>⚠ 不建議結束 {p.command}</strong>
              <p>{warn.info.advice_note}</p>
              {#if force}<p>你勾選了「強制結束」,程式沒有機會正常收尾,風險更高。</p>{/if}
              <div class="warn-actions">
                <button onclick={() => (warn = null)}>取消</button>
                <button class="danger" onclick={() => doKill(warn.port)}>我了解風險,仍要結束</button>
              </div>
            </td>
          </tr>
        {/if}
        {#if expanded === rowKey(p)}
          {@const d = details[p.pid]}
          <tr class="detail">
            <td colspan="5">
              {#if !d || d.loading}
                讀取中…
              {:else if d.error}
                <span class="error">{d.error}</span>
              {:else}
                <p class="hint">
                  {d.data.hint ?? "沒有內建說明。可從下方的執行檔路徑、指令列與工作目錄判斷它屬於哪個專案或軟體。"}
                  {#if d.data.custom_id}<span class="badge">你的自訂說明</span>{/if}
                </p>
                <div class="advice {d.data.advice}">
                  <strong>{d.data.advice_label}</strong>
                  <span>{d.data.advice_note}</span>
                </div>
                <dl>
                  <dt>執行檔</dt><dd>{d.data.exe}</dd>
                  <dt>指令列</dt><dd>{d.data.args}</dd>
                  <dt>工作目錄</dt><dd>{d.data.cwd || "—"}</dd>
                  <dt>使用者</dt><dd>{d.data.user}</dd>
                  <dt>啟動時間</dt><dd>{d.data.started}</dd>
                </dl>
                <div class="custom-actions">
                  {#if d.data.custom_id}
                    <button onclick={() => openForm(p, d.data, rules.find((r) => r.id === d.data.custom_id))}>編輯自訂說明</button>
                    <button onclick={() => removeRule(d.data.custom_id)}>刪除自訂說明</button>
                  {:else}
                    <button onclick={() => openForm(p, d.data, null)}>＋ 自訂說明</button>
                  {/if}
                </div>
                {#if form}
                  <form class="rule-form" onsubmit={(e) => { e.preventDefault(); saveForm(); }}>
                    <label>比對方式
                      <select bind:value={form.kind} onchange={() => (form.pattern = form.defaults[form.kind])}>
                        {#each Object.entries(KIND_LABEL) as [k, label]}<option value={k}>{label}</option>{/each}
                      </select>
                    </label>
                    <label>比對內容<input bind:value={form.pattern} /></label>
                    <label>這是什麼<input bind:value={form.desc} placeholder="例如:我的 AI 交易機器人" /></label>
                    <label>建議
                      <select bind:value={form.advice}>
                        {#each Object.entries(ADVICE_LABEL) as [k, label]}<option value={k}>{label}</option>{/each}
                      </select>
                    </label>
                    <label>關閉影響(選填)<input bind:value={form.note} placeholder="例如:關閉後機器人停止下單" /></label>
                    {#if form.error}<p class="error">{form.error}</p>{/if}
                    <div class="warn-actions">
                      <button type="button" onclick={() => (form = null)}>取消</button>
                      <button type="submit" class="primary">儲存</button>
                    </div>
                  </form>
                {/if}
              {/if}
            </td>
          </tr>
        {/if}
      {:else}
        <tr><td colspan="5" class="empty">沒有符合的 port</td></tr>
      {/each}
    </tbody>
  </table>

  <footer>
    <span>顯示 / 隱藏快捷鍵</span>
    <kbd class:recording>{recording ? "請按下新的組合鍵…" : pretty(shortcut)}</kbd>
    <button onclick={() => { recording = !recording; shortcutError = ""; }}>
      {recording ? "取消 (Esc)" : "變更"}
    </button>
    {#if shortcutError}<span class="error">{shortcutError}</span>{/if}
  </footer>

  <details class="rules">
    <summary>自訂說明({rules.length})</summary>
    {#if rulesError}<p class="error">{rulesError}</p>{/if}
    {#each rules as r (r.id)}
      <div class="rule">
        <span class="muted">{KIND_LABEL[r.kind]}</span>
        <code>{r.pattern}</code>
        <span>→ {r.desc}({ADVICE_LABEL[r.advice]})</span>
        <button onclick={() => removeRule(r.id)}>刪除</button>
      </div>
    {:else}
      <p class="muted">還沒有自訂說明。展開任一列,按「＋ 自訂說明」即可新增。</p>
    {/each}
  </details>
</main>

<style>
  main {
    padding: 16px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 12px;
    flex-wrap: wrap;
  }
  h1 {
    font-size: 18px;
    margin: 0 8px 0 0;
  }
  input[type="search"] {
    flex: 1;
    min-width: 160px;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--panel);
    color: var(--text);
  }
  label {
    color: var(--muted);
    white-space: nowrap;
  }
  button {
    padding: 4px 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--panel);
    color: var(--text);
    cursor: pointer;
  }
  button:hover {
    border-color: var(--accent);
  }
  button.danger {
    background: var(--danger);
    border-color: var(--danger);
    color: #fff;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  th,
  td {
    text-align: left;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
  }
  th {
    color: var(--muted);
    font-weight: 500;
  }
  .port {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .muted {
    color: var(--muted);
  }
  .action {
    text-align: right;
  }
  .empty {
    text-align: center;
    color: var(--muted);
  }
  button.link {
    border: none;
    background: none;
    padding: 0;
    text-align: left;
    font: inherit;
  }
  button.link:hover {
    color: var(--accent);
  }
  .warn td {
    background: color-mix(in srgb, var(--danger) 12%, var(--panel));
    border-left: 4px solid var(--danger);
  }
  .warn strong {
    color: var(--danger);
  }
  .warn p {
    margin: 4px 0;
  }
  .warn-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 8px;
  }
  button:disabled {
    opacity: 0.6;
    cursor: wait;
  }
  .custom-actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  .badge {
    margin-left: 6px;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--accent);
    color: #fff;
    font-size: 12px;
    font-weight: 400;
  }
  .rule-form {
    display: grid;
    gap: 6px;
    margin-top: 8px;
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--panel);
  }
  .rule-form label {
    display: grid;
    grid-template-columns: 110px 1fr;
    align-items: center;
    gap: 8px;
  }
  .rule-form input,
  .rule-form select {
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .rules {
    margin-top: 12px;
    color: var(--muted);
  }
  .rules summary {
    cursor: pointer;
  }
  .rule {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 0;
    flex-wrap: wrap;
  }
  .rule code {
    color: var(--text);
    word-break: break-all;
  }
  .detail td {
    background: var(--bg);
    font-size: 13px;
  }
  .hint {
    margin: 0 0 8px;
    font-weight: 500;
  }
  .advice {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0 0 8px;
    padding: 8px 10px;
    border-left: 4px solid var(--muted);
    border-radius: 4px;
    background: var(--panel);
  }
  .advice.safe {
    border-left-color: #2e9d57;
  }
  .advice.safe strong {
    color: #2e9d57;
  }
  .advice.caution {
    border-left-color: #d9961a;
  }
  .advice.caution strong {
    color: #d9961a;
  }
  .advice.avoid {
    border-left-color: var(--danger);
  }
  .advice.avoid strong {
    color: var(--danger);
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 2px 12px;
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    word-break: break-all;
    user-select: text;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 12px;
    color: var(--muted);
    flex-wrap: wrap;
  }
  kbd {
    padding: 2px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--panel);
    color: var(--text);
    font: inherit;
  }
  kbd.recording {
    border-color: var(--accent);
    color: var(--accent);
  }
  .error {
    color: var(--danger);
  }
  .ok {
    color: var(--accent);
  }
</style>
