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
          {line}
        </span>
      ))}
    </span>
  );
}
