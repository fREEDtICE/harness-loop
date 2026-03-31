export function PromptEditor({
  testid,
  title,
  value,
  onChange,
}: {
  testid: string;
  title: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <section className="prompt-editor" data-testid={testid}>
      <div className="prompt-editor-header">
        <h3>{title}</h3>
      </div>
      <textarea data-testid={`${testid}-textarea`} value={value} onChange={(event) => onChange(event.target.value)} rows={9} />
    </section>
  );
}

export function StatCard({
  testid,
  label,
  value,
  detail,
}: {
  testid: string;
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <article className="stat-card" data-testid={testid}>
      <span>{label}</span>
      <strong>{value}</strong>
      <p>{detail}</p>
    </article>
  );
}

export function StageRow({ testid, heading, text }: { testid?: string; heading: string; text: string }) {
  return (
    <div className="stage-row" data-testid={testid}>
      <strong>{heading}</strong>
      <span>{text}</span>
    </div>
  );
}
