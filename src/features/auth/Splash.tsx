import "./auth.css";

/** Shown for the brief moment before the app knows whether an account exists. Mirrors `splash.dart` (a static centered logo — its own rotation controller is created but never actually attached to anything, so mobile's splash has no real animation either). */
export function Splash() {
  return (
    <div className="auth-screen">
      <img src="/onboarding/logo.png" alt="" className="splash__logo" />
    </div>
  );
}
