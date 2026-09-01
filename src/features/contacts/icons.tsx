/**
 * Icon set for `ContactProfile` — each is a thin wrapper around a
 * standalone `.svg` file in `src/assets/icons/`, loaded via Vite's `?raw`
 * import and injected inline (see `components/Icon.tsx`) so replacing the
 * file is all that's needed to change how it looks; no code changes.
 */
import { Icon } from "../../components/Icon";
import userCheckSvg from "../../assets/icons/user-check.svg?raw";
import userMinusSvg from "../../assets/icons/user-minus.svg?raw";
import userPlusSvg from "../../assets/icons/user-plus.svg?raw";
import unlockSvg from "../../assets/icons/unlock.svg?raw";
import chatBubbleSvg from "../../assets/icons/chat-bubble.svg?raw";
import phoneSvg from "../../assets/icons/phone.svg?raw";
import checkSvg from "../../assets/icons/check.svg?raw";
import createAliasChatSvg from "../../assets/icons/create-alias-chat.svg?raw";
import copySvg from "../../assets/icons/copy.svg?raw";
import blockContactSvg from "../../assets/icons/block-contact.svg?raw";

type IconProps = { className?: string };

export function IconUserCheck({ className }: IconProps) {
  return <Icon svg={userCheckSvg} className={className} />;
}

export function IconUserMinus({ className }: IconProps) {
  return <Icon svg={userMinusSvg} className={className} />;
}

export function IconUserPlus({ className }: IconProps) {
  return <Icon svg={userPlusSvg} className={className} />;
}

export function IconLockOpen({ className }: IconProps) {
  return <Icon svg={unlockSvg} className={className} />;
}

export function IconChatBubble({ className }: IconProps) {
  return <Icon svg={chatBubbleSvg} className={className} />;
}

export function IconPhone({ className }: IconProps) {
  return <Icon svg={phoneSvg} className={className} />;
}

export function IconCheck({ className }: IconProps) {
  return <Icon svg={checkSvg} className={className} />;
}

export function IconCreateAliasChat({ className }: IconProps) {
  return <Icon svg={createAliasChatSvg} className={className} />;
}

export function IconCopy({ className }: IconProps) {
  return <Icon svg={copySvg} className={className} />;
}

export function IconBlock({ className }: IconProps) {
  return <Icon svg={blockContactSvg} className={className} />;
}
