/**
 * Icon set for the nav drawer's menu items — each is a thin wrapper around
 * a standalone `.svg` file in `src/assets/icons/`, loaded via Vite's `?raw`
 * import and injected inline (see `components/Icon.tsx`) so replacing the
 * file is all that's needed to change how it looks; no code changes.
 */
import { Icon } from "../../components/Icon";
import navChatsSvg from "../../assets/icons/nav-chats.svg?raw";
import navContactsSvg from "../../assets/icons/nav-contacts.svg?raw";
import navFilesSvg from "../../assets/icons/nav-files.svg?raw";
import navSettingsSvg from "../../assets/icons/nav-settings.svg?raw";

type IconProps = { className?: string };

export function IconChats({ className }: IconProps) {
  return <Icon svg={navChatsSvg} className={className} />;
}

export function IconContacts({ className }: IconProps) {
  return <Icon svg={navContactsSvg} className={className} />;
}

export function IconFiles({ className }: IconProps) {
  return <Icon svg={navFilesSvg} className={className} />;
}

export function IconSettings({ className }: IconProps) {
  return <Icon svg={navSettingsSvg} className={className} />;
}
