import { ContactsSidebar } from "../chat/ContactsSidebar";
import { ChatWindow } from "../chat/ChatWindow";
import { ContactProfile } from "../contacts/ContactProfile";
import { ContactsListSidebar } from "../contacts/ContactsListSidebar";
import { NavDrawer } from "./NavDrawer";
import { useChatUiStore } from "../../stores/chatUiStore";
import "../chat/chat.css";

export function MainLayout() {
  const leftPanel = useChatUiStore((s) => s.leftPanel);
  const viewingContactWallet = useChatUiStore((s) => s.viewingContactWallet);

  return (
    <div className="main-layout">
      {leftPanel === "contacts" ? <ContactsListSidebar /> : <ContactsSidebar />}
      {viewingContactWallet ? <ContactProfile /> : <ChatWindow />}
      <NavDrawer />
    </div>
  );
}
