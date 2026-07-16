/** Minimal argv helpers — no external CLI framework. */

export function flag(argv: string[], name: string): boolean {
  return argv.includes(name);
}

export function opt(argv: string[], name: string, fallback?: string): string | undefined {
  const i = argv.indexOf(name);
  if (i >= 0 && argv[i + 1] && !argv[i + 1]!.startsWith("-")) return argv[i + 1];
  const long = argv.find((a) => a.startsWith(`${name}=`));
  if (long) return long.slice(name.length + 1);
  return fallback;
}

export function envOr(name: string, fallback: string): string {
  return process.env[name] && process.env[name]!.length > 0 ? process.env[name]! : fallback;
}
