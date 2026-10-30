import type { SVGProps } from "react";

type IconName =
  | "book"
  | "clock"
  | "sparkles"
  | "history"
  | "pulse"
  | "volume"
  | "copy"
  | "pin"
  | "close"
  | "chevron"
  | "check"
  | "search"
  | "pause"
  | "play"
  | "folder"
  | "key"
  | "font"
  | "arrowDown"
  | "alert";

const paths: Record<IconName, React.ReactNode> = {
  book: (
    <>
      <path d="M4 5.5A2.5 2.5 0 0 1 6.5 3H11v16H6.5A2.5 2.5 0 0 0 4 21.5v-16Z" />
      <path d="M20 5.5A2.5 2.5 0 0 0 17.5 3H13v16h4.5a2.5 2.5 0 0 1 2.5 2.5v-16Z" />
    </>
  ),
  clock: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </>
  ),
  sparkles: (
    <>
      <path d="m12 3 1.25 3.75L17 8l-3.75 1.25L12 13l-1.25-3.75L7 8l3.75-1.25L12 3Z" />
      <path d="m5.5 14 .75 2.25L8.5 17l-2.25.75L5.5 20l-.75-2.25L2.5 17l2.25-.75L5.5 14Z" />
      <path d="m18 14 .75 2.25L21 17l-2.25.75L18 20l-.75-2.25L15 17l2.25-.75L18 14Z" />
    </>
  ),
  history: (
    <>
      <path d="M4 4v5h5" />
      <path d="M5.6 17.4A8 8 0 1 0 4 9" />
      <path d="M12 8v5l3 2" />
    </>
  ),
  pulse: <path d="M3 12h4l2-6 4 12 2-6h6" />,
  volume: (
    <>
      <path d="M5 10v4h3l4 3V7L8 10H5Z" />
      <path d="M16 9a4 4 0 0 1 0 6" />
      <path d="M18.5 6.5a8 8 0 0 1 0 11" />
    </>
  ),
  copy: (
    <>
      <rect x="9" y="9" width="10" height="11" rx="2" />
      <path d="M15 9V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3" />
    </>
  ),
  pin: (
    <>
      <path d="m9 3 6 6" />
      <path d="m14 4 6 6-3 1-3 3-1 3-6-6 3-1 3-3 1-3Z" />
      <path d="m9 15-5 5" />
    </>
  ),
  close: (
    <>
      <path d="m6 6 12 12" />
      <path d="m18 6-12 12" />
    </>
  ),
  chevron: <path d="m8 10 4 4 4-4" />,
  check: <path d="m5 12 4 4L19 6" />,
  search: (
    <>
      <circle cx="11" cy="11" r="7" />
      <path d="m16 16 5 5" />
    </>
  ),
  pause: (
    <>
      <path d="M9 7v10" />
      <path d="M15 7v10" />
    </>
  ),
  play: <path d="m9 6 9 6-9 6V6Z" />,
  folder: (
    <path d="M3 6a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6Z" />
  ),
  key: (
    <>
      <circle cx="8" cy="15" r="4" />
      <path d="m11 12 8-8" />
      <path d="m16 7 2 2" />
      <path d="m14 9 2 2" />
    </>
  ),
  font: (
    <>
      <path d="M4 20 9 5h2l5 15" />
      <path d="M6 15h8" />
      <path d="M16 10h4" />
    </>
  ),
  arrowDown: <path d="M12 4v16m-6-6 6 6 6-6" />,
  alert: (
    <>
      <path d="M12 3 2.5 20h19L12 3Z" />
      <path d="M12 9v5" />
      <path d="M12 17h.01" />
    </>
  ),
};

interface IconProps extends SVGProps<SVGSVGElement> {
  name: IconName;
  size?: number;
}

export function Icon({ name, size = 18, ...props }: IconProps) {
  return (
    <svg
      aria-hidden="true"
      fill="none"
      height={size}
      viewBox="0 0 24 24"
      width={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      {...props}
    >
      {paths[name]}
    </svg>
  );
}
