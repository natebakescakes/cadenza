import { useEffect, useState, type ReactNode } from "react";
import { motion } from "framer-motion";
import { KeyRound, Loader2, Lock, ShieldCheck } from "lucide-react";
import { LogoMark } from "@/components/Logo";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { dbInit, dbDevUnlock, dbUnlock, isDbInitialized, startLogging } from "@/lib/api";
import { useLoggingState } from "@/hooks/useLoggingState";

/**
 * Gate the whole app behind an encrypted-DB unlock screen.
 * First run → set a password (db_init). Subsequent runs → unlock (db_unlock).
 * Renders children only once the DB reports unlocked.
 */
export function DbGate({ children }: { children: ReactNode }) {
  const { state, setUnlocked } = useLoggingState();
  const [initialized, setInitialized] = useState<boolean | null>(null);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    // Dev bypass: skip password gate entirely in Vite dev mode.
    if (import.meta.env.DEV) {
      dbDevUnlock()
        .then((ok) => {
          if (ok) {
            setUnlocked(true);
            startLogging().catch(() => {});
          } else {
            // Backend refused (shouldn't happen in dev) — fall through to normal gate.
            isDbInitialized().then(setInitialized).catch(() => setInitialized(false));
          }
        })
        .catch(() => {
          // Fall back to normal gate on any error.
          isDbInitialized().then(setInitialized).catch(() => setInitialized(false));
        });
      return;
    }
    isDbInitialized()
      .then(setInitialized)
      .catch(() => setInitialized(false));
  }, [setUnlocked]);

  const unlocked = state.db_unlocked;

  const submit = async () => {
    setErr(null);
    const isFirstRun = initialized === false;
    if (isFirstRun) {
      if (password.length < 4) return setErr("Choose at least 4 characters.");
      if (password !== confirm) return setErr("Passwords don't match.");
    } else if (!password) {
      return setErr("Enter your password.");
    }
    setBusy(true);
    try {
      if (isFirstRun) {
        await dbInit(password);
        setUnlocked(true);
        startLogging().catch(() => {});
      } else {
        const ok = await dbUnlock(password);
        if (ok) {
          setUnlocked(true);
          startLogging().catch(() => {});
        } else {
          setErr("Incorrect password. Try again.");
        }
      }
    } catch {
      setErr("Something went wrong unlocking the vault.");
    } finally {
      setBusy(false);
    }
  };

  if (unlocked) return <>{children}</>;

  const firstRun = initialized === false;

  return (
    <div className="app-canvas grid h-full place-items-center px-6">
      <motion.div
        initial={{ opacity: 0, y: 14, scale: 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        transition={{ duration: 0.5, ease: [0.16, 1, 0.3, 1] }}
        className="w-full max-w-sm"
      >
        <div className="mb-6 flex flex-col items-center text-center">
          <div className="mb-4 grid size-14 place-items-center rounded-2xl border border-border bg-secondary/50 text-gold">
            <LogoMark size={30} />
          </div>
          <h1 className="font-display text-2xl font-semibold tracking-tight text-foreground">
            {initialized === null
              ? "Cadenza"
              : firstRun
                ? "Secure your vault"
                : "Welcome back"}
          </h1>
          <p className="mt-1.5 max-w-xs text-sm text-muted-foreground">
            {firstRun
              ? "Your keystrokes are encrypted on this device. Set a password to protect them."
              : "Enter your password to unlock your encrypted typing vault."}
          </p>
        </div>

        {initialized === null ? (
          <div className="flex justify-center py-6 text-muted-foreground">
            <Loader2 className="size-5 animate-spin" />
          </div>
        ) : (
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void submit();
            }}
            className="space-y-3"
          >
            <div className="space-y-1.5">
              <Label htmlFor="db-pw" className="text-xs text-muted-foreground">
                Password
              </Label>
              <div className="relative">
                <Lock className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input
                  id="db-pw"
                  type="password"
                  autoFocus
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder="••••••••"
                  className="pl-9"
                />
              </div>
            </div>

            {firstRun && (
              <div className="space-y-1.5">
                <Label htmlFor="db-confirm" className="text-xs text-muted-foreground">
                  Confirm password
                </Label>
                <div className="relative">
                  <KeyRound className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    id="db-confirm"
                    type="password"
                    value={confirm}
                    onChange={(e) => setConfirm(e.target.value)}
                    placeholder="••••••••"
                    className="pl-9"
                  />
                </div>
              </div>
            )}

            {err && (
              <p className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">
                {err}
              </p>
            )}

            <Button type="submit" className="w-full" disabled={busy} size="lg">
              {busy ? (
                <Loader2 className="size-4 animate-spin" />
              ) : firstRun ? (
                <ShieldCheck className="size-4" />
              ) : (
                <Lock className="size-4" />
              )}
              {firstRun ? "Create vault & continue" : "Unlock"}
            </Button>
          </form>
        )}
      </motion.div>
    </div>
  );
}
