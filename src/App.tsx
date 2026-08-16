import { useEffect } from "react";
import { useSessionStore } from "./stores/sessionStore";
import { AuthFlow } from "./features/auth/AuthFlow";
import { LockScreen } from "./features/auth/LockScreen";
import { MainLayout } from "./features/layout/MainLayout";
import { useMessagesUpdatedListener } from "./hooks/useMessagesUpdatedListener";
import "./styles/theme.css";

function App() {
  const status = useSessionStore((s) => s.status);
  const bootstrap = useSessionStore((s) => s.bootstrap);
  const pendingMnemonic = useSessionStore((s) => s.pendingMnemonic);

  useEffect(() => {
    bootstrap();
  }, [bootstrap]);

  useMessagesUpdatedListener();

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
