function formatPhonetic(value: string): string {
  if (
    value.startsWith("/") ||
    value.startsWith("[") ||
    value.startsWith("英音 ") ||
    value.startsWith("美音 ")
  ) {
    return value;
  }
  let clean = value;
  while (clean.startsWith("/") || clean.startsWith("[")) {
    clean = clean.slice(1);
  }
  while (clean.endsWith("/") || clean.endsWith("]")) {
    clean = clean.slice(0, -1);
  }
  return "/" + clean + "/";
}

interface PhoneticLinesProps {
  value?: string;
  className?: string;
}

export function PhoneticLines({ value, className }: PhoneticLinesProps) {
  const lines = (value ?? "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  if (lines.length === 0) return null;
  return (
    <span className={"phonetic-stack" + (className ? " " + className : "")}>
      {lines.map((line, index) => (
        <span className="phonetic-line" key={line + "-" + index}>
          {formatPhonetic(line)}
        </span>
      ))}
    </span>
  );
}
