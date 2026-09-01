import { useEffect } from "react";
import { useSessionStore } from "./stores/sessionStore";
import { useUpdateStore } from "./stores/updateStore";
import { AuthFlow } from "./features/auth/AuthFlow";
import { LockScreen } from "./features/auth/LockScreen";
import { MainLayout } from "./features/layout/MainLayout";
import { useMessagesUpdatedListener } from "./hooks/useMessagesUpdatedListener";
import "./styles/theme.css";

function App() {
  const status = useSessionStore((s) => s.status);
  const bootstrap = useSessionStore((s) => s.bootstrap);
  const pendingMnemonic = useSessionStore((s) => s.pendingMnemonic);
  const checkForUpdate = useUpdateStore((s) => s.check);
  const updateChecked = useUpdateStore((s) => s.checked);

  useEffect(() => {
    bootstrap();
  }, [bootstrap]);

  useMessagesUpdatedListener();

  // Once, the first time the app reaches an unlocked state — not gated
  // behind opening the hamburger menu itself, so the banner is already
  // known-available (or not) the moment the user does open it, instead of
  // showing a beat of "checking…" every single time.
  useEffect(() => {
    if (status === "unlocked" && !updateChecked) {
      checkForUpdate();
    }
  }, [status, updateChecked, checkForUpdate]);

  if (status === "unknown") {
    return (
      <div style={{ height: "100vh", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--color-text-secondary)" }}>
        Loading…
      </div>
    );
  }

  if (status === "noAccount") return <AuthFlow />;
  if (status === "locked") return <LockScreen />;

  // "unlocked" — but if we just created a brand-new wallet, AuthFlow still
  // owns the screen until the user confirms they've backed up the phrase.
  if (pendingMnemonic) return <AuthFlow />;

  return <MainLayout />;
}

export default App;
