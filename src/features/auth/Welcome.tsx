import "./auth.css";

interface WelcomeProps {
  onContinue: () => void;
}

/** First screen of the onboarding flow — mirrors `welcome.dart`'s copy, laid out as a centered card with a hero row (graphic + repeated tagline) per the desktop mockup. */
export function Welcome({ onContinue }: WelcomeProps) {
  return (
    <div className="auth-screen welcome-screen-bg">
      <div className="welcome-card">
        <p className="welcome-card__eyebrow">Welcome to</p>
        <img src="/onboarding/logo.png" alt="Sealed" className="welcome-card__logo" />
        <h1 className="welcome-card__title">Private messages. Zero surveillance.</h1>
        <p className="welcome-card__subhead">End-to-end encrypted. No phone number,  no email, no servers watching.</p>

        <div className="welcome-card__hero">
          <img src="/onboarding/svg/welcome_graphic.svg" alt="" className="welcome-card__hero-graphic" />
          <div className="welcome-card__hero-text">
            <h2 className="welcome-card__hero-title">Private messages. Zero surveillance.</h2>
            <p className="welcome-card__hero-line">End-to-end encrypted.</p>
            <p className="welcome-card__hero-line">No phone number,</p>
            <p className="welcome-card__hero-line">no email,</p>
            <p className="welcome-card__hero-line">no servers watching.</p>
          </div>
        </div>

        <button className="btn btn--primary welcome-card__cta" onClick={onContinue}>
          Create account
        </button>
      </div>
    </div>
  );
}
