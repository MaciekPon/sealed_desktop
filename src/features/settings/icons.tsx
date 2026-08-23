/**
 * Icon set for the Settings list rows — each is a thin wrapper around a
 * standalone `.svg` file in `src/assets/icons/`, loaded via Vite's `?raw`
 * import and injected inline (see `components/Icon.tsx`) so replacing the
 * file is all that's needed to change how it looks; no code changes.
 */
import { Icon } from "../../components/Icon";
import usernameSvg from "../../assets/icons/username.svg?raw";
import bioSvg from "../../assets/icons/bio.svg?raw";
import walletSvg from "../../assets/icons/wallet.svg?raw";
import creditsSvg from "../../assets/icons/credits.svg?raw";
import redeemSvg from "../../assets/icons/redeem.svg?raw";
import topupSvg from "../../assets/icons/topup.svg?raw";
import notificationSvg from "../../assets/icons/notification.svg?raw";
import passwordSvg from "../../assets/icons/password.svg?raw";
import terminationSvg from "../../assets/icons/termination.svg?raw";
import autoDeleteSvg from "../../assets/icons/auto-delete.svg?raw";
import resyncSvg from "../../assets/icons/resync.svg?raw";
import falconSvg from "../../assets/icons/falcon.svg?raw";
import spamFilterSvg from "../../assets/icons/spam-filter.svg?raw";
import recoveryKeySvg from "../../assets/icons/recovery-key.svg?raw";
import logoutSvg from "../../assets/icons/logout.svg?raw";
import chevronRightSvg from "../../assets/icons/chevron-right.svg?raw";

type IconProps = { className?: string };

export function IconAt({ className }: IconProps) {
  return <Icon svg={usernameSvg} className={className} />;
}

export function IconPencil({ className }: IconProps) {
  return <Icon svg={bioSvg} className={className} />;
}

export function IconWallet({ className }: IconProps) {
  return <Icon svg={walletSvg} className={className} />;
}

export function IconInfo({ className }: IconProps) {
  return <Icon svg={creditsSvg} className={className} />;
}

export function IconStar({ className }: IconProps) {
  return <Icon svg={redeemSvg} className={className} />;
}

export function IconPlus({ className }: IconProps) {
  return <Icon svg={topupSvg} className={className} />;
}

export function IconBell({ className }: IconProps) {
  return <Icon svg={notificationSvg} className={className} />;
}

export function IconLock({ className }: IconProps) {
  return <Icon svg={passwordSvg} className={className} />;
}

export function IconShieldOff({ className }: IconProps) {
  return <Icon svg={terminationSvg} className={className} />;
}

export function IconTrash({ className }: IconProps) {
  return <Icon svg={autoDeleteSvg} className={className} />;
}

export function IconRefresh({ className }: IconProps) {
  return <Icon svg={resyncSvg} className={className} />;
}

export function IconAtom({ className }: IconProps) {
  return <Icon svg={falconSvg} className={className} />;
}

export function IconFilter({ className }: IconProps) {
  return <Icon svg={spamFilterSvg} className={className} />;
}

export function IconKey({ className }: IconProps) {
  return <Icon svg={recoveryKeySvg} className={className} />;
}

export function IconPower({ className }: IconProps) {
  return <Icon svg={logoutSvg} className={className} />;
}

export function IconChevronRight({ className }: IconProps) {
  return <Icon svg={chevronRightSvg} className={className} />;
}
