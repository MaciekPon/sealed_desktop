import { formatMessageTimestamp } from "../../lib/format";
import "./chat.css";

/** Structural subset shared by `DecryptedMessage` (wallet DMs) and `AliasMessage` — this is all a bubble ever needs. */
interface BubbleMessage {
  content: string;
  isOutgoing: boolean;
  timestamp: number;
}

export function MessageBubble({ message }: { message: BubbleMessage }) {
  return (
    <div className={`bubble ${message.isOutgoing ? "bubble--outgoing" : "bubble--incoming"}`}>
      {message.content}
      <span className="bubble__meta">
        {formatMessageTimestamp(message.timestamp)}
        {message.isOutgoing && " ✓"}
      </span>
    </div>
  );
}
