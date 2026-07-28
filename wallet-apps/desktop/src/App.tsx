import { FormEvent, useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { QRCodeSVG } from "qrcode.react";
import type { ActionResponse, Page, Theme, WalletSnapshot } from "./types";

const pages: Array<{ id: Page; label: string; icon: string }> = [
  { id: "overview", label: "Overview", icon: "▦" },
  { id: "send", label: "Send", icon: "↗" },
  { id: "receive", label: "Receive", icon: "↙" },
  { id: "activity", label: "Activity", icon: "◴" },
  { id: "security", label: "Security", icon: "◇" },
];

function applyTheme(theme: Theme) {
  const resolved = theme === "system"
    ? (matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark")
    : theme;
  document.documentElement.dataset.theme = resolved;
  localStorage.setItem("grid.wallet.theme", theme);
}

export function App() {
  const [snapshot, setSnapshot] = useState<WalletSnapshot>();
  const [page, setPage] = useState<Page>("overview");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [phrase, setPhrase] = useState("");
  const [theme, setTheme] = useState<Theme>(
    () => (localStorage.getItem("grid.wallet.theme") as Theme) || "system",
  );

  const refresh = useCallback(async () => {
    setBusy(true);
    try {
      setSnapshot(await invoke<WalletSnapshot>("wallet_snapshot"));
      setError("");
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  }, []);

  const act = useCallback(async (action: Record<string, unknown>) => {
    setBusy(true);
    setError("");
    try {
      const result = await invoke<ActionResponse>("wallet_action", { action });
      setSnapshot(result.snapshot);
      setMessage(result.message);
      if (result.recoveryPhrase) setPhrase(result.recoveryPhrase);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => { applyTheme(theme); }, [theme]);
  useEffect(() => { void refresh(); }, [refresh]);

  return (
    <div className="app">
      <aside>
        <div className="brand"><span className="mark">▦</span><div><b>EMBER</b><small>GRID WALLET</small></div></div>
        <nav>{pages.map((item) => (
          <button key={item.id} className={page === item.id ? "active" : ""} onClick={() => setPage(item.id)}>
            <span>{item.icon}</span>{item.label}
          </button>
        ))}</nav>
        <div className="vault-state"><i className={snapshot?.auth.unlocked ? "ok" : ""} />
          {snapshot?.auth.unlocked ? "Vault unlocked" : "Vault locked"}
        </div>
      </aside>
      <main>
        <header><span>GRID → SOL → BTC</span><button onClick={() => void refresh()} disabled={busy}>↻ Refresh</button></header>
        {page === "overview" && <Overview data={snapshot} act={act} />}
        {page === "send" && <Send data={snapshot} act={act} />}
        {page === "receive" && <Receive data={snapshot} />}
        {page === "activity" && <Activity data={snapshot} />}
        {page === "security" && <Security data={snapshot} act={act} theme={theme} setTheme={setTheme} />}
        {busy && <div className="loading">Working with the local GRID chain…</div>}
        {message && <div className="toast" onClick={() => setMessage("")}>{message}</div>}
        {error && <div className="toast error" onClick={() => setError("")}>{error}</div>}
      </main>
      {phrase && <Recovery phrase={phrase} close={() => setPhrase("")} />}
    </div>
  );
}

type Action = (action: Record<string, unknown>) => Promise<void>;

function Heading({ label, title, body }: { label: string; title: string; body: string }) {
  return <div className="heading"><small>{label}</small><h1>{title}</h1><p>{body}</p></div>;
}

function Metric({ label, value, detail }: { label: string; value: string; detail: string }) {
  return <div className="metric"><small>{label}</small><strong>{value}</strong><p>{detail}</p></div>;
}

function Overview({ data, act }: { data?: WalletSnapshot; act: Action }) {
  if (!data) return null;
  return <section>
    <Heading label="GRID → SOL → BTC" title="One wallet. Three layers." body="GRID utility, Solana settlement, Bitcoin transaction security." />
    <div className="metrics">
      <Metric label="GRID CHAIN" value={data.grid.balance.toFixed(2)} detail={data.grid.address || "Wallet not initialized"} />
      <Metric label="MINING REWARDS" value={`${data.grid.unclaimed.toFixed(2)} GRID`} detail={`Claim before ${data.grid.burnDeadlineDays} days`} />
      <Metric label={`SOLANA ${data.solana.network}`} value={data.solana.balance == null ? "Not linked" : `${data.solana.balance.toFixed(2)} GRID`} detail={data.solana.address || "Create a reward wallet"} />
    </div>
    <div className="route">
      <Layer active={data.grid.initialized} name="GRID" detail="Compute utility" /><b>→</b>
      <Layer active={data.solana.configured} name="SOL" detail="Fast settlement" /><b>→</b>
      <Layer active={data.bitcoin.live} name="BTC" detail="Security + exit" />
      <span className="roadmap">{data.bitcoin.live ? "LIVE" : "CONSOLIDATION ROADMAP"}</span>
    </div>
    {!data.auth.initialized && <Setup act={act} />}
    {data.auth.initialized && !data.grid.initialized && <Callout title="Create your GRID wallet" body="Derive a grid0 address from your protected operator key." label="Initialize wallet" click={() => act({ action: "initializeGrid" })} />}
    {data.grid.unclaimed > 0 && <Callout title="Mining rewards are ready" body={`${data.grid.unclaimed.toFixed(2)} GRID is waiting on-chain.`} label="Claim all" click={() => act({ action: "claim" })} />}
  </section>;
}

function Layer({ active, name, detail }: { active: boolean; name: string; detail: string }) {
  return <div className="layer"><i className={active ? "ok" : ""} /><div><b>{name}</b><small>{detail}</small></div></div>;
}

function Callout({ title, body, label, click }: { title: string; body: string; label: string; click: () => void }) {
  return <div className="callout"><div><h3>{title}</h3><p>{body}</p></div><button className="primary" onClick={click}>{label}</button></div>;
}

function Setup({ act }: { act: Action }) {
  const [mode, setMode] = useState("keyphrase");
  const [password, setPassword] = useState("");
  const submit = () => {
    const action: Record<string, unknown> = { action: `setup${mode[0].toUpperCase()}${mode.slice(1)}` };
    if (mode === "password" || mode === "combo") action.password = password;
    void act(action);
  };
  return <div className="callout setup"><div><h3>Protect your GRID wallet</h3><p>Use the real encrypted GRID vault and recovery workflow.</p>
    <select value={mode} onChange={(event) => setMode(event.target.value)}>
      <option value="keyphrase">24-word recovery phrase</option><option value="passkey">Passkey</option>
      <option value="password">Password</option><option value="combo">Password + passkey + phrase</option>
    </select>
    {(mode === "password" || mode === "combo") && <input type="password" value={password} onChange={(event) => setPassword(event.target.value)} placeholder="Wallet password" />}
  </div><button className="primary" onClick={submit} disabled={(mode === "password" || mode === "combo") && !password}>Create vault</button></div>;
}

function Send({ data, act }: { data?: WalletSnapshot; act: Action }) {
  const [to, setTo] = useState(""); const [amount, setAmount] = useState(""); const [memo, setMemo] = useState("");
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const value = Number(amount);
    if (!window.confirm(`Send ${value.toFixed(6)} GRID to ${to}? This transaction cannot be undone.`)) return;
    void act({ action: "send", to, amount: value, memo });
  };
  return <section><Heading label="GRID CHAIN" title="Send GRID" body="Signed locally with your protected operator key." />
    <form className="form" onSubmit={submit}><label>Recipient<input value={to} onChange={(e) => setTo(e.target.value)} placeholder="grid0…" /></label>
      <label>Amount<input value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" placeholder="0.00" /></label>
      <label>Memo<input value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="Optional" /></label>
      <p>Available <b>{data?.grid.balance.toFixed(6) || "0"} GRID</b></p>
      <button className="primary" disabled={!to || !(Number(amount) > 0) || !data?.auth.unlocked}>Review and send</button>
    </form></section>;
}

function Receive({ data }: { data?: WalletSnapshot }) {
  const [rail, setRail] = useState<"grid" | "solana">("grid");
  const address = rail === "grid" ? data?.grid.address : data?.solana.address;
  const label = rail === "grid" ? "GRID chain address" : `Solana ${data?.solana.network || "devnet"} reward address`;
  const missing = rail === "grid"
    ? "Initialize your GRID wallet first"
    : "Create or import a Solana reward wallet in Security";

  return <section><Heading label="RECEIVE" title="Receive funds" body="Choose the correct network, then share only this public address." />
    <div className="receive">
      <div className="segments receive-tabs" role="group" aria-label="Receive network">
        <button className={rail === "grid" ? "active" : ""} onClick={() => setRail("grid")}>GRID</button>
        <button className={rail === "solana" ? "active" : ""} onClick={() => setRail("solana")}>Solana rewards</button>
      </div>
      <small>{label}</small>
      {address ? <>
        <div className="qr" aria-label={`QR code for ${label}`}>
          <QRCodeSVG value={address} size={220} level="M" marginSize={3} />
        </div>
        <code>{address}</code>
        <button onClick={() => navigator.clipboard.writeText(address)}>Copy address</button>
        <p>Scan or copy this address. Confirm the network before sending.</p>
      </> : <p className="empty-address">{missing}</p>}
    </div>
  </section>;
}

function Activity({ data }: { data?: WalletSnapshot }) {
  return <section><Heading label="LOCAL CHAIN" title="Activity" body="Claims, sends, receives, mints, and protocol burns." />
    <div className="transactions">{data?.activity.length ? data.activity.map((tx) => <div key={tx.id}>
      <b>{tx.kind.toUpperCase()}</b><span>{tx.memo || tx.id}</span><strong>{tx.amount.toFixed(6)} GRID</strong><time>{tx.at.slice(0, 19)}</time>
    </div>) : <p className="empty">No transactions yet.</p>}</div></section>;
}

function Security({ data, act, theme, setTheme }: { data?: WalletSnapshot; act: Action; theme: Theme; setTheme: (theme: Theme) => void }) {
  const [password, setPassword] = useState(""); const [keyphrase, setKeyphrase] = useState(""); const [address, setAddress] = useState("");
  const [networkMode, setNetworkMode] = useState("genesis");
  const [truthUrl, setTruthUrl] = useState("");
  const [p2pPeer, setP2pPeer] = useState("");
  useEffect(() => {
    if (!data?.network) return;
    setNetworkMode(data.network.mode);
    setTruthUrl(data.network.truthUrl);
    setP2pPeer(data.network.p2pPeer);
  }, [data?.network]);
  return <section><Heading label="CUSTODY" title="Security & settlement" body="Vault protection, appearance, and Solana reward routing." />
    <div className="settings"><article><h3>Appearance</h3><div className="segments">{(["system", "light", "dark"] as Theme[]).map((value) =>
      <button className={theme === value ? "active" : ""} key={value} onClick={() => setTheme(value)}>{value}</button>)}</div></article>
      <article><h3>GRID network</h3>
        <p><i className={data?.network.connected && data.network.trusted ? "ok" : ""} /> {data?.network.connected && data.network.trusted ? `Connected · block ${data.network.height ?? 0}` : data?.network.error || "Connecting to Genesis…"}</p>
        <select value={networkMode} onChange={(event) => setNetworkMode(event.target.value)}>
          <option value="genesis">GRID Genesis (recommended)</option>
          <option value="local">Local node</option>
          <option value="custom">Custom node</option>
        </select>
        {networkMode === "custom" ? <>
          <input value={truthUrl} onChange={(event) => setTruthUrl(event.target.value)} placeholder="http://host:9100" />
          <input value={p2pPeer} onChange={(event) => setP2pPeer(event.target.value)} placeholder="host:9900" />
        </> : null}
        <button onClick={() => void act({ action: "setNetwork", mode: networkMode, truthUrl, p2pPeer })}>Save network</button>
        <p className="font-mono">{data?.network.truthUrl || "http://genesis.grid-compute.com:9100"}<br />{data?.network.p2pPeer || "genesis.grid-compute.com:9900"}</p>
      </article>
      <article><h3>GRID vault</h3><p>{data?.auth.detail}</p>{!data?.auth.initialized ? <Setup act={act} /> : !data.auth.unlocked && <>
        {(data.auth.mode === "password" || data.auth.mode === "combo") && <input type="password" placeholder="Password" value={password} onChange={(e) => setPassword(e.target.value)} />}
        {(data.auth.mode === "keyphrase" || data.auth.mode === "combo") && <textarea placeholder="24-word recovery phrase" value={keyphrase} onChange={(e) => setKeyphrase(e.target.value)} />}
        <button className="primary" onClick={() => { void act({ action: "unlock", password, keyphrase }); setPassword(""); setKeyphrase(""); }}>Unlock vault</button>
      </>}</article>
      <article><h3>Solana mining rewards</h3>{data?.solana.address ? <><code>{data.solana.address}</code><p>{data.solana.balance?.toFixed(6) || "—"} GRID · devnet</p></> :
        <><button className="primary" onClick={() => void act({ action: "createSolana" })}>Create reward wallet</button>
          <div className="inline"><input placeholder="Or import a public address" value={address} onChange={(e) => setAddress(e.target.value)} /><button onClick={() => void act({ action: "importSolana", address })}>Import</button></div></>}</article>
      <article><h3>Bitcoin consolidation layer</h3><p><b>GRID → SOL → BTC</b></p><p>Direct conversion remains clearly labeled roadmap until audited liquidity and execution are live.</p></article>
    </div></section>;
}

function Recovery({ phrase, close }: { phrase: string; close: () => void }) {
  const [saved, setSaved] = useState(false);
  return <div className="modal"><div><span className="key">⚿</span><h2>Your 24-word recovery phrase</h2><p>Write these words down in order and store them offline. GRID will not show them again.</p>
    <code className="phrase">{phrase}</code><label className="check"><input type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} /> I saved the phrase offline</label>
    <button className="primary" disabled={!saved} onClick={close}>Continue</button></div></div>;
}
