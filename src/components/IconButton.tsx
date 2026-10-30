import type { ButtonHTMLAttributes } from "react";
import { Icon } from "./Icon";

type IconName = Parameters<typeof Icon>[0]["name"];

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: IconName;
  label: string;
  active?: boolean;
}

export function IconButton({
  icon,
  label,
  active = false,
  className = "",
  ...props
}: IconButtonProps) {
  return (
    <button
      aria-label={label}
      className={`icon-button ${active ? "is-active" : ""} ${className}`}
      title={label}
      type="button"
      {...props}
    >
      <Icon name={icon} />
    </button>
  );
}
