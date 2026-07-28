export type WalletSnapshot = {
  version: number;
  configDir: string;
  auth: {
    initialized: boolean;
    mode: string;
    encrypted: boolean;
    unlocked: boolean;
    passkeyRegistered: boolean;
    detail: string;
  };
  grid: {
    initialized: boolean;
    address: string | null;
    balance: number;
    unclaimed: number;
    totalMinted: number;
    totalBurned: number;
    maxSupply: number;
    burnDeadlineDays: number;
  };
  solana: {
    configured: boolean;
    address: string | null;
    balance: number | null;
    network: string;
    custody: string | null;
    mint: string;
    error: string | null;
  };
  bitcoin: { network: string; role: string; route: string; live: boolean };
  network: {
    mode: string;
    truthUrl: string;
    p2pPeer: string;
    connected: boolean;
    trusted: boolean;
    chainId: string | null;
    height: number | null;
    leaderPubkey: string | null;
    error: string | null;
  };
  activity: Array<{
    id: string;
    kind: string;
    at: string;
    from?: string;
    to?: string;
    amount: number;
    memo?: string;
  }>;
};

export type ActionResponse = {
  ok: boolean;
  message: string;
  recoveryPhrase?: string;
  snapshot: WalletSnapshot;
};

export type Page = "overview" | "send" | "receive" | "activity" | "security";
export type Theme = "system" | "light" | "dark";
