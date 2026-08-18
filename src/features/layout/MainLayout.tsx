import { ContactsSidebar } from "../chat/ContactsSidebar";
import { ChatWindow } from "../chat/ChatWindow";
import { ContactProfile } from "../contacts/ContactProfile";
import { ContactsListScreen } from "../contacts/ContactsListScreen";
import { NavDrawer } from "./NavDrawer";
import { useChatUiStore } from "../../stores/chatUiStore";
import "../chat/chat.css";

export function MainLayout() {
  const screen = useChatUiStore((s) => s.screen);

  return (
    <div className="main-layout">
      <ContactsSidebar />
      {screen === "contactProfile" && <ContactProfile />}
      {screen === "contactsList" && <ContactsListScreen />}
      {screen === "chat" && <ChatWindow />}
      <NavDrawer />
    </div>
  );
}
