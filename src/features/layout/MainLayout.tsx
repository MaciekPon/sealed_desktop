import { ContactsSidebar } from "../chat/ContactsSidebar";
import { ChatWindow } from "../chat/ChatWindow";
import { SettingsScreen } from "../settings/SettingsScreen";
import { ContactProfile } from "../contacts/ContactProfile";
import { AliasScreen } from "../alias/AliasScreen";
import { useChatUiStore } from "../../stores/chatUiStore";
import "../chat/chat.css";

export function MainLayout() {
  const screen = useChatUiStore((s) => s.screen);

  return (
    <div className="main-layout">
      <ContactsSidebar />
      {screen === "settings" && <SettingsScreen />}
      {screen === "contactProfile" && <ContactProfile />}
      {screen === "alias" && <AliasScreen />}
      {screen === "chat" && <ChatWindow />}
    </div>
  );
}
