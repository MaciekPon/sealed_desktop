import { useState } from "react";
import "./auth.css";

interface Step {
  video: string;
  title: string;
  tags: string[];
  body: (string | { text: string; highlight: true })[];
}

/** Mirrors `how_it_works.dart`'s `_steps` — same copy, same 4 looping-video assets (already boomerang-looped, so `loop` alone plays back-and-forth with no visible seam). */
const STEPS: Step[] = [
  {
    video: "/onboarding/animations/account_loop.mp4",
    title: "Create Wallet-Based Account",
    tags: ["Anonymous Conversation", "Decentralized"],
    body: ["Create an account directly from your blockchain wallet. No phone number, email, or personal data required."],
  },
  {
    video: "/onboarding/animations/coins_loop.mp4",
    title: "Add Network Credits",
    tags: ["Blockchain Based", "No Data Collected"],
    body: ["Fund your account and pay only for message delivery. No intermediary servers, no access to your conversations."],
  },
  {
    video: "/onboarding/animations/msg_loop.mp4",
    title: "Send Secure Messages",
    tags: ["Encrypted", "Direct Communication"],
    body: [
      "Top up your account and start messaging instantly. Communicate through a ",
      { text: "privacy-first", highlight: true },
      " ",
      { text: "decentralized network.", highlight: true },
    ],
  },
  {
    video: "/onboarding/animations/opensource_loop.mp4",
    title: "Don't Trust, Verify",
    tags: ["Open Source Verified", "Transparent by Design"],
    body: [
      "Our protocol is fully open source and publicly auditable. Review the code, verify the architecture, and validate our privacy claims yourself.",
    ],
  },
];

interface HowItWorksProps {
  onDone: () => void;
}

/** Second onboarding screen — a 4-step carousel, mirrors `how_it_works.dart`'s `HowItWorksFlow`. */
export function HowItWorks({ onDone }: HowItWorksProps) {
  const [index, setIndex] = useState(0);
  const isLast = index === STEPS.length - 1;
  const step = STEPS[index];

  function next() {
    if (isLast) {
      onDone();
      return;
    }
    setIndex((i) => i + 1);
  }

  return (
    <div className="auth-screen welcome-screen-bg how-it-works">
      <h1 className="welcome-screen__eyebrow-title">How it works?</h1>
      <div className="how-it-works__hero">
        <div className="how-it-works__video-frame">
          <video key={step.video} className="how-it-works__video" src={step.video} autoPlay loop muted playsInline />
        </div>
        <div className="how-it-works__hero-text">
          <div className="how-it-works__tags">
            {step.tags.map((tag) => (
              <span key={tag} className="how-it-works__tag">
                {tag}
              </span>
            ))}
          </div>
          <p className="how-it-works__body">
            {step.body.map((part, i) =>
              typeof part === "string" ? (
                <span key={i}>{part}</span>
              ) : (
                <span key={i} className="how-it-works__body-highlight">
                  {part.text}
                </span>
              ),
            )}
          </p>
        </div>
      </div>
      <button className="btn btn--primary how-it-works__cta" onClick={next}>
        Next
      </button>
      <div className="how-it-works__dots">
        {STEPS.map((s, i) => (
          <span key={s.video} className={`how-it-works__dot ${i === index ? "how-it-works__dot--active" : ""}`} />
        ))}
      </div>
    </div>
  );
}
